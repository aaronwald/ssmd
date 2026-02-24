use serde::{Deserialize, Serialize};

use crate::error::TransitionError;

/// States an order can be in
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    /// Order created, queued for submission
    Pending,
    /// Sent to exchange, awaiting acknowledgement
    Submitted,
    /// Exchange acknowledged, resting on book
    Acknowledged,
    /// Partially filled
    PartiallyFilled,
    /// Completely filled (terminal)
    Filled,
    /// Cancel request sent
    PendingCancel,
    /// Successfully cancelled (terminal)
    Cancelled,
    /// Rejected by exchange or risk check (terminal)
    Rejected,
    /// Expired by exchange (terminal)
    Expired,
}

impl OrderState {
    /// Whether this is a terminal state (no further transitions allowed)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            OrderState::Filled
                | OrderState::Cancelled
                | OrderState::Rejected
                | OrderState::Expired
        )
    }

    /// Whether this order is considered "open" (contributing to risk)
    pub fn is_open(&self) -> bool {
        matches!(
            self,
            OrderState::Pending
                | OrderState::Submitted
                | OrderState::Acknowledged
                | OrderState::PartiallyFilled
                | OrderState::PendingCancel
        )
    }
}

impl std::fmt::Display for OrderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            OrderState::Pending => "pending",
            OrderState::Submitted => "submitted",
            OrderState::Acknowledged => "acknowledged",
            OrderState::PartiallyFilled => "partially_filled",
            OrderState::Filled => "filled",
            OrderState::PendingCancel => "pending_cancel",
            OrderState::Cancelled => "cancelled",
            OrderState::Rejected => "rejected",
            OrderState::Expired => "expired",
        };
        write!(f, "{}", s)
    }
}

/// Events that trigger state transitions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderEvent {
    /// Order submitted to exchange
    Submit,
    /// Exchange acknowledged the order (resting on book)
    Acknowledge { exchange_order_id: String },
    /// Exchange rejected the order
    Reject { reason: String },
    /// Partial fill received
    PartialFill { filled_qty: i32 },
    /// Complete fill received
    Fill { filled_qty: i32 },
    /// Cancel requested by user or system
    CancelRequest,
    /// Cancel confirmed by exchange
    CancelConfirm,
    /// Order expired
    Expire,
}

impl std::fmt::Display for OrderEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderEvent::Submit => write!(f, "submit"),
            OrderEvent::Acknowledge { .. } => write!(f, "acknowledge"),
            OrderEvent::Reject { .. } => write!(f, "reject"),
            OrderEvent::PartialFill { .. } => write!(f, "partial_fill"),
            OrderEvent::Fill { .. } => write!(f, "fill"),
            OrderEvent::CancelRequest => write!(f, "cancel_request"),
            OrderEvent::CancelConfirm => write!(f, "cancel_confirm"),
            OrderEvent::Expire => write!(f, "expire"),
        }
    }
}

/// Apply an event to the current state, returning the new state.
///
/// This is a pure function with no side effects - the core of the state machine.
pub fn apply_event(
    current: OrderState,
    event: &OrderEvent,
) -> Result<OrderState, TransitionError> {
    // Terminal states reject all events
    if current.is_terminal() {
        return Err(TransitionError::InvalidTransition {
            from: current,
            event: event.to_string(),
            reason: "order is in terminal state".to_string(),
        });
    }

    match (current, event) {
        // Pending transitions
        (OrderState::Pending, OrderEvent::Submit) => Ok(OrderState::Submitted),
        (OrderState::Pending, OrderEvent::Reject { .. }) => Ok(OrderState::Rejected),

        // Submitted transitions
        (OrderState::Submitted, OrderEvent::Acknowledge { .. }) => Ok(OrderState::Acknowledged),
        (OrderState::Submitted, OrderEvent::Reject { .. }) => Ok(OrderState::Rejected),
        (OrderState::Submitted, OrderEvent::Fill { .. }) => Ok(OrderState::Filled),
        (OrderState::Submitted, OrderEvent::PartialFill { .. }) => {
            Ok(OrderState::PartiallyFilled)
        }

        // Acknowledged transitions
        (OrderState::Acknowledged, OrderEvent::PartialFill { .. }) => {
            Ok(OrderState::PartiallyFilled)
        }
        (OrderState::Acknowledged, OrderEvent::Fill { .. }) => Ok(OrderState::Filled),
        (OrderState::Acknowledged, OrderEvent::CancelRequest) => Ok(OrderState::PendingCancel),
        (OrderState::Acknowledged, OrderEvent::Expire) => Ok(OrderState::Expired),

        // PartiallyFilled transitions
        (OrderState::PartiallyFilled, OrderEvent::Fill { .. }) => Ok(OrderState::Filled),
        (OrderState::PartiallyFilled, OrderEvent::PartialFill { .. }) => {
            Ok(OrderState::PartiallyFilled)
        }
        (OrderState::PartiallyFilled, OrderEvent::CancelRequest) => {
            Ok(OrderState::PendingCancel)
        }

        // PendingCancel transitions
        (OrderState::PendingCancel, OrderEvent::CancelConfirm) => Ok(OrderState::Cancelled),
        // Fill can win the race against cancel
        (OrderState::PendingCancel, OrderEvent::Fill { .. }) => Ok(OrderState::Filled),
        // Partial fill while cancel is pending: stay in PendingCancel to preserve cancel intent.
        // The filled_quantity is updated in the DB, but the cancel request remains active.
        (OrderState::PendingCancel, OrderEvent::PartialFill { .. }) => {
            Ok(OrderState::PendingCancel)
        }

        // All other transitions are invalid
        (state, evt) => Err(TransitionError::InvalidTransition {
            from: state,
            event: evt.to_string(),
            reason: format!("event {} not valid in state {}", evt, state),
        }),
    }
}

/// Resolve a local order state against exchange state.
///
/// Returns `Some(new_state)` for deterministic resolutions, `None` when
/// special handling is needed (e.g., PendingCancel + Resting requires
/// re-sending the cancel). Used by both recovery and reconciliation.
pub fn resolve_exchange_state(
    local_state: &OrderState,
    exchange_state: &crate::types::ExchangeOrderState,
) -> Option<OrderState> {
    use crate::types::ExchangeOrderState;
    match (local_state, exchange_state) {
        (OrderState::Submitted, ExchangeOrderState::Resting) => Some(OrderState::Acknowledged),
        (OrderState::Submitted, ExchangeOrderState::Executed) => Some(OrderState::Filled),
        (OrderState::Submitted, ExchangeOrderState::NotFound) => Some(OrderState::Rejected),
        (OrderState::Submitted, ExchangeOrderState::Cancelled) => Some(OrderState::Cancelled),
        (OrderState::PendingCancel, ExchangeOrderState::Cancelled) => Some(OrderState::Cancelled),
        (OrderState::PendingCancel, ExchangeOrderState::Executed) => Some(OrderState::Filled),
        (OrderState::PendingCancel, ExchangeOrderState::NotFound) => Some(OrderState::Cancelled),
        // PendingCancel + Resting: needs special handling (re-send cancel)
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Valid transitions ===

    #[test]
    fn test_pending_to_submitted() {
        let result = apply_event(OrderState::Pending, &OrderEvent::Submit);
        assert_eq!(result.unwrap(), OrderState::Submitted);
    }

    #[test]
    fn test_pending_to_rejected() {
        let result = apply_event(
            OrderState::Pending,
            &OrderEvent::Reject {
                reason: "risk check".to_string(),
            },
        );
        assert_eq!(result.unwrap(), OrderState::Rejected);
    }

    #[test]
    fn test_submitted_to_acknowledged() {
        let result = apply_event(
            OrderState::Submitted,
            &OrderEvent::Acknowledge {
                exchange_order_id: "exch-123".to_string(),
            },
        );
        assert_eq!(result.unwrap(), OrderState::Acknowledged);
    }

    #[test]
    fn test_submitted_to_rejected() {
        let result = apply_event(
            OrderState::Submitted,
            &OrderEvent::Reject {
                reason: "invalid ticker".to_string(),
            },
        );
        assert_eq!(result.unwrap(), OrderState::Rejected);
    }

    #[test]
    fn test_submitted_to_filled() {
        let result = apply_event(
            OrderState::Submitted,
            &OrderEvent::Fill { filled_qty: 10 },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_submitted_to_partially_filled() {
        let result = apply_event(
            OrderState::Submitted,
            &OrderEvent::PartialFill { filled_qty: 5 },
        );
        assert_eq!(result.unwrap(), OrderState::PartiallyFilled);
    }

    #[test]
    fn test_acknowledged_to_partially_filled() {
        let result = apply_event(
            OrderState::Acknowledged,
            &OrderEvent::PartialFill { filled_qty: 5 },
        );
        assert_eq!(result.unwrap(), OrderState::PartiallyFilled);
    }

    #[test]
    fn test_acknowledged_to_filled() {
        let result = apply_event(
            OrderState::Acknowledged,
            &OrderEvent::Fill { filled_qty: 10 },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_acknowledged_to_pending_cancel() {
        let result = apply_event(OrderState::Acknowledged, &OrderEvent::CancelRequest);
        assert_eq!(result.unwrap(), OrderState::PendingCancel);
    }

    #[test]
    fn test_acknowledged_to_expired() {
        let result = apply_event(OrderState::Acknowledged, &OrderEvent::Expire);
        assert_eq!(result.unwrap(), OrderState::Expired);
    }

    #[test]
    fn test_partially_filled_to_filled() {
        let result = apply_event(
            OrderState::PartiallyFilled,
            &OrderEvent::Fill { filled_qty: 5 },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_partially_filled_to_partially_filled() {
        let result = apply_event(
            OrderState::PartiallyFilled,
            &OrderEvent::PartialFill { filled_qty: 3 },
        );
        assert_eq!(result.unwrap(), OrderState::PartiallyFilled);
    }

    #[test]
    fn test_partially_filled_to_pending_cancel() {
        let result = apply_event(OrderState::PartiallyFilled, &OrderEvent::CancelRequest);
        assert_eq!(result.unwrap(), OrderState::PendingCancel);
    }

    #[test]
    fn test_pending_cancel_to_cancelled() {
        let result = apply_event(OrderState::PendingCancel, &OrderEvent::CancelConfirm);
        assert_eq!(result.unwrap(), OrderState::Cancelled);
    }

    #[test]
    fn test_pending_cancel_fill_wins_race() {
        let result = apply_event(
            OrderState::PendingCancel,
            &OrderEvent::Fill { filled_qty: 10 },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_pending_cancel_partial_fill_preserves_cancel() {
        // Partial fill during PendingCancel should stay PendingCancel to preserve cancel intent
        let result = apply_event(
            OrderState::PendingCancel,
            &OrderEvent::PartialFill { filled_qty: 3 },
        );
        assert_eq!(result.unwrap(), OrderState::PendingCancel);
    }

    // === Invalid transitions (terminal states reject all) ===

    #[test]
    fn test_filled_rejects_all() {
        assert!(apply_event(OrderState::Filled, &OrderEvent::Submit).is_err());
        assert!(apply_event(
            OrderState::Filled,
            &OrderEvent::Acknowledge {
                exchange_order_id: "x".into()
            }
        )
        .is_err());
        assert!(apply_event(OrderState::Filled, &OrderEvent::CancelRequest).is_err());
        assert!(apply_event(OrderState::Filled, &OrderEvent::CancelConfirm).is_err());
        assert!(apply_event(OrderState::Filled, &OrderEvent::Fill { filled_qty: 1 }).is_err());
    }

    #[test]
    fn test_cancelled_rejects_all() {
        assert!(apply_event(OrderState::Cancelled, &OrderEvent::Submit).is_err());
        assert!(apply_event(OrderState::Cancelled, &OrderEvent::Fill { filled_qty: 1 }).is_err());
        assert!(apply_event(OrderState::Cancelled, &OrderEvent::CancelConfirm).is_err());
    }

    #[test]
    fn test_rejected_rejects_all() {
        assert!(apply_event(OrderState::Rejected, &OrderEvent::Submit).is_err());
        assert!(apply_event(OrderState::Rejected, &OrderEvent::Fill { filled_qty: 1 }).is_err());
    }

    #[test]
    fn test_expired_rejects_all() {
        assert!(apply_event(OrderState::Expired, &OrderEvent::Submit).is_err());
        assert!(apply_event(OrderState::Expired, &OrderEvent::Fill { filled_qty: 1 }).is_err());
        assert!(apply_event(OrderState::Expired, &OrderEvent::CancelRequest).is_err());
    }

    // === Invalid transitions (wrong event for state) ===

    #[test]
    fn test_pending_rejects_cancel() {
        assert!(apply_event(OrderState::Pending, &OrderEvent::CancelRequest).is_err());
    }

    #[test]
    fn test_pending_rejects_acknowledge() {
        assert!(apply_event(
            OrderState::Pending,
            &OrderEvent::Acknowledge {
                exchange_order_id: "x".into()
            }
        )
        .is_err());
    }

    #[test]
    fn test_pending_rejects_fill() {
        assert!(apply_event(OrderState::Pending, &OrderEvent::Fill { filled_qty: 1 }).is_err());
    }

    #[test]
    fn test_submitted_rejects_cancel_request() {
        assert!(apply_event(OrderState::Submitted, &OrderEvent::CancelRequest).is_err());
    }

    #[test]
    fn test_submitted_rejects_cancel_confirm() {
        assert!(apply_event(OrderState::Submitted, &OrderEvent::CancelConfirm).is_err());
    }

    #[test]
    fn test_acknowledged_rejects_submit() {
        assert!(apply_event(OrderState::Acknowledged, &OrderEvent::Submit).is_err());
    }

    #[test]
    fn test_acknowledged_rejects_reject() {
        assert!(apply_event(
            OrderState::Acknowledged,
            &OrderEvent::Reject {
                reason: "too late".into()
            }
        )
        .is_err());
    }

    // === State properties ===

    #[test]
    fn test_terminal_states() {
        assert!(OrderState::Filled.is_terminal());
        assert!(OrderState::Cancelled.is_terminal());
        assert!(OrderState::Rejected.is_terminal());
        assert!(OrderState::Expired.is_terminal());

        assert!(!OrderState::Pending.is_terminal());
        assert!(!OrderState::Submitted.is_terminal());
        assert!(!OrderState::Acknowledged.is_terminal());
        assert!(!OrderState::PartiallyFilled.is_terminal());
        assert!(!OrderState::PendingCancel.is_terminal());
    }

    #[test]
    fn test_open_states() {
        assert!(OrderState::Pending.is_open());
        assert!(OrderState::Submitted.is_open());
        assert!(OrderState::Acknowledged.is_open());
        assert!(OrderState::PartiallyFilled.is_open());
        assert!(OrderState::PendingCancel.is_open());

        assert!(!OrderState::Filled.is_open());
        assert!(!OrderState::Cancelled.is_open());
        assert!(!OrderState::Rejected.is_open());
        assert!(!OrderState::Expired.is_open());
    }

    #[test]
    fn test_state_display() {
        assert_eq!(OrderState::Pending.to_string(), "pending");
        assert_eq!(OrderState::Submitted.to_string(), "submitted");
        assert_eq!(OrderState::Acknowledged.to_string(), "acknowledged");
        assert_eq!(OrderState::PartiallyFilled.to_string(), "partially_filled");
        assert_eq!(OrderState::Filled.to_string(), "filled");
        assert_eq!(OrderState::PendingCancel.to_string(), "pending_cancel");
        assert_eq!(OrderState::Cancelled.to_string(), "cancelled");
        assert_eq!(OrderState::Rejected.to_string(), "rejected");
        assert_eq!(OrderState::Expired.to_string(), "expired");
    }
}
