use rust_decimal::Decimal;
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
    /// Amend request sent (price and/or quantity change)
    PendingAmend,
    /// Decrease request sent (quantity reduction, preserves queue priority)
    PendingDecrease,
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
                | OrderState::PendingAmend
                | OrderState::PendingDecrease
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
            OrderState::PendingAmend => "pending_amend",
            OrderState::PendingDecrease => "pending_decrease",
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
    PartialFill { filled_qty: Decimal },
    /// Complete fill received
    Fill { filled_qty: Decimal },
    /// Cancel requested by user or system
    CancelRequest,
    /// Cancel confirmed by exchange
    CancelConfirm,
    /// Amend requested by user or system
    AmendRequest,
    /// Amend confirmed by exchange
    AmendConfirm,
    /// Decrease requested by user or system
    DecreaseRequest,
    /// Decrease confirmed by exchange
    DecreaseConfirm,
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
            OrderEvent::AmendRequest => write!(f, "amend_request"),
            OrderEvent::AmendConfirm => write!(f, "amend_confirm"),
            OrderEvent::DecreaseRequest => write!(f, "decrease_request"),
            OrderEvent::DecreaseConfirm => write!(f, "decrease_confirm"),
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
        // === Fills are ALWAYS accepted from any non-terminal state ===
        // Exchanges may deliver fills out of order (e.g., fill before ack).
        // We must never reject a fill — that would leave the DB diverged from the exchange.
        (_, OrderEvent::Fill { .. }) => Ok(OrderState::Filled),
        (OrderState::PendingCancel, OrderEvent::PartialFill { .. }) => {
            // Partial fill while cancel is pending: stay in PendingCancel to preserve cancel intent.
            // The filled_quantity is updated in the DB, but the cancel request remains active.
            Ok(OrderState::PendingCancel)
        }
        (OrderState::PendingAmend, OrderEvent::PartialFill { .. }) => {
            // Partial fill while amend is pending: stay in PendingAmend to preserve amend intent.
            Ok(OrderState::PendingAmend)
        }
        (OrderState::PendingDecrease, OrderEvent::PartialFill { .. }) => {
            // Partial fill while decrease is pending: stay in PendingDecrease to preserve intent.
            Ok(OrderState::PendingDecrease)
        }
        (_, OrderEvent::PartialFill { .. }) => Ok(OrderState::PartiallyFilled),

        // Pending transitions
        (OrderState::Pending, OrderEvent::Submit) => Ok(OrderState::Submitted),
        (OrderState::Pending, OrderEvent::Reject { .. }) => Ok(OrderState::Rejected),

        // Submitted transitions
        (OrderState::Submitted, OrderEvent::Acknowledge { .. }) => Ok(OrderState::Acknowledged),
        (OrderState::Submitted, OrderEvent::Reject { .. }) => Ok(OrderState::Rejected),
        (OrderState::Submitted, OrderEvent::Expire) => Ok(OrderState::Expired),

        // Acknowledged transitions
        (OrderState::Acknowledged, OrderEvent::CancelRequest) => Ok(OrderState::PendingCancel),
        (OrderState::Acknowledged, OrderEvent::AmendRequest) => Ok(OrderState::PendingAmend),
        (OrderState::Acknowledged, OrderEvent::DecreaseRequest) => Ok(OrderState::PendingDecrease),
        (OrderState::Acknowledged, OrderEvent::Expire) => Ok(OrderState::Expired),

        // PartiallyFilled transitions
        (OrderState::PartiallyFilled, OrderEvent::CancelRequest) => {
            Ok(OrderState::PendingCancel)
        }
        (OrderState::PartiallyFilled, OrderEvent::AmendRequest) => {
            Ok(OrderState::PendingAmend)
        }
        (OrderState::PartiallyFilled, OrderEvent::DecreaseRequest) => {
            Ok(OrderState::PendingDecrease)
        }

        // PendingCancel transitions
        (OrderState::PendingCancel, OrderEvent::CancelConfirm) => Ok(OrderState::Cancelled),

        // PendingAmend transitions
        (OrderState::PendingAmend, OrderEvent::AmendConfirm) => Ok(OrderState::Acknowledged),
        (OrderState::PendingAmend, OrderEvent::CancelRequest) => Ok(OrderState::PendingCancel),

        // PendingDecrease transitions
        (OrderState::PendingDecrease, OrderEvent::DecreaseConfirm) => Ok(OrderState::Acknowledged),
        (OrderState::PendingDecrease, OrderEvent::CancelRequest) => Ok(OrderState::PendingCancel),

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
        // PendingAmend/PendingDecrease: exchange says it's done → Acknowledged
        (OrderState::PendingAmend, ExchangeOrderState::Resting) => Some(OrderState::Acknowledged),
        (OrderState::PendingAmend, ExchangeOrderState::Executed) => Some(OrderState::Filled),
        (OrderState::PendingAmend, ExchangeOrderState::Cancelled) => Some(OrderState::Cancelled),
        (OrderState::PendingDecrease, ExchangeOrderState::Resting) => Some(OrderState::Acknowledged),
        (OrderState::PendingDecrease, ExchangeOrderState::Executed) => Some(OrderState::Filled),
        (OrderState::PendingDecrease, ExchangeOrderState::Cancelled) => Some(OrderState::Cancelled),
        // PendingCancel + Resting: needs special handling (re-send cancel)
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ======================================================================
    // Core invariant: fills are ALWAYS accepted from any non-terminal state.
    // "We always must accept a fill that we get"
    // ======================================================================

    #[test]
    fn test_fill_accepted_from_pending() {
        // Fill before ack — exchange filled before we even got acknowledgement
        let result = apply_event(OrderState::Pending, &OrderEvent::Fill { filled_qty: Decimal::from(10) });
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_fill_accepted_from_submitted() {
        let result = apply_event(
            OrderState::Submitted,
            &OrderEvent::Fill { filled_qty: Decimal::from(10) },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_fill_accepted_from_acknowledged() {
        let result = apply_event(
            OrderState::Acknowledged,
            &OrderEvent::Fill { filled_qty: Decimal::from(10) },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_fill_accepted_from_partially_filled() {
        let result = apply_event(
            OrderState::PartiallyFilled,
            &OrderEvent::Fill { filled_qty: Decimal::from(5) },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_fill_accepted_from_pending_cancel() {
        // Fill wins the race against cancel
        let result = apply_event(
            OrderState::PendingCancel,
            &OrderEvent::Fill { filled_qty: Decimal::from(10) },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    /// Exhaustive: every non-terminal state accepts Fill → Filled
    #[test]
    fn test_fill_accepted_from_all_non_terminal_states() {
        let non_terminal = [
            OrderState::Pending,
            OrderState::Submitted,
            OrderState::Acknowledged,
            OrderState::PartiallyFilled,
            OrderState::PendingCancel,
            OrderState::PendingAmend,
            OrderState::PendingDecrease,
        ];
        for state in non_terminal {
            let result = apply_event(state, &OrderEvent::Fill { filled_qty: Decimal::from(1) });
            assert_eq!(
                result.unwrap(),
                OrderState::Filled,
                "Fill must be accepted from {:?}",
                state
            );
        }
    }

    // ======================================================================
    // Partial fills: accepted from all non-terminal states.
    // PendingCancel + PartialFill stays PendingCancel (preserves cancel intent).
    // ======================================================================

    #[test]
    fn test_partial_fill_from_pending() {
        // Partial fill before ack
        let result = apply_event(
            OrderState::Pending,
            &OrderEvent::PartialFill { filled_qty: Decimal::from(3) },
        );
        assert_eq!(result.unwrap(), OrderState::PartiallyFilled);
    }

    #[test]
    fn test_partial_fill_from_submitted() {
        let result = apply_event(
            OrderState::Submitted,
            &OrderEvent::PartialFill { filled_qty: Decimal::from(5) },
        );
        assert_eq!(result.unwrap(), OrderState::PartiallyFilled);
    }

    #[test]
    fn test_partial_fill_from_acknowledged() {
        let result = apply_event(
            OrderState::Acknowledged,
            &OrderEvent::PartialFill { filled_qty: Decimal::from(5) },
        );
        assert_eq!(result.unwrap(), OrderState::PartiallyFilled);
    }

    #[test]
    fn test_partial_fill_from_partially_filled() {
        let result = apply_event(
            OrderState::PartiallyFilled,
            &OrderEvent::PartialFill { filled_qty: Decimal::from(3) },
        );
        assert_eq!(result.unwrap(), OrderState::PartiallyFilled);
    }

    #[test]
    fn test_partial_fill_from_pending_cancel_preserves_cancel() {
        // Partial fill during PendingCancel: stay PendingCancel to preserve cancel intent
        let result = apply_event(
            OrderState::PendingCancel,
            &OrderEvent::PartialFill { filled_qty: Decimal::from(3) },
        );
        assert_eq!(result.unwrap(), OrderState::PendingCancel);
    }

    /// Exhaustive: every non-terminal state accepts PartialFill
    #[test]
    fn test_partial_fill_accepted_from_all_non_terminal_states() {
        let non_terminal = [
            OrderState::Pending,
            OrderState::Submitted,
            OrderState::Acknowledged,
            OrderState::PartiallyFilled,
            OrderState::PendingCancel,
            OrderState::PendingAmend,
            OrderState::PendingDecrease,
        ];
        for state in non_terminal {
            let result = apply_event(state, &OrderEvent::PartialFill { filled_qty: Decimal::from(1) });
            assert!(
                result.is_ok(),
                "PartialFill must be accepted from {:?}",
                state
            );
        }
    }

    // ======================================================================
    // Standard transitions (non-fill)
    // ======================================================================

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
    fn test_submitted_to_expired() {
        let result = apply_event(OrderState::Submitted, &OrderEvent::Expire);
        assert_eq!(result.unwrap(), OrderState::Expired);
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
    fn test_partially_filled_to_pending_cancel() {
        let result = apply_event(OrderState::PartiallyFilled, &OrderEvent::CancelRequest);
        assert_eq!(result.unwrap(), OrderState::PendingCancel);
    }

    #[test]
    fn test_pending_cancel_to_cancelled() {
        let result = apply_event(OrderState::PendingCancel, &OrderEvent::CancelConfirm);
        assert_eq!(result.unwrap(), OrderState::Cancelled);
    }

    // ======================================================================
    // Terminal states reject ALL events (exhaustive)
    // ======================================================================

    #[test]
    fn test_filled_rejects_all_events() {
        let events: Vec<OrderEvent> = vec![
            OrderEvent::Submit,
            OrderEvent::Acknowledge { exchange_order_id: "x".into() },
            OrderEvent::Reject { reason: "r".into() },
            OrderEvent::PartialFill { filled_qty: Decimal::from(1) },
            OrderEvent::Fill { filled_qty: Decimal::from(1) },
            OrderEvent::CancelRequest,
            OrderEvent::CancelConfirm,
            OrderEvent::Expire,
        ];
        for event in &events {
            assert!(
                apply_event(OrderState::Filled, event).is_err(),
                "Filled must reject {:?}",
                event
            );
        }
    }

    #[test]
    fn test_cancelled_rejects_all_events() {
        let events: Vec<OrderEvent> = vec![
            OrderEvent::Submit,
            OrderEvent::Acknowledge { exchange_order_id: "x".into() },
            OrderEvent::Reject { reason: "r".into() },
            OrderEvent::PartialFill { filled_qty: Decimal::from(1) },
            OrderEvent::Fill { filled_qty: Decimal::from(1) },
            OrderEvent::CancelRequest,
            OrderEvent::CancelConfirm,
            OrderEvent::Expire,
        ];
        for event in &events {
            assert!(
                apply_event(OrderState::Cancelled, event).is_err(),
                "Cancelled must reject {:?}",
                event
            );
        }
    }

    #[test]
    fn test_rejected_rejects_all_events() {
        let events: Vec<OrderEvent> = vec![
            OrderEvent::Submit,
            OrderEvent::Acknowledge { exchange_order_id: "x".into() },
            OrderEvent::Reject { reason: "r".into() },
            OrderEvent::PartialFill { filled_qty: Decimal::from(1) },
            OrderEvent::Fill { filled_qty: Decimal::from(1) },
            OrderEvent::CancelRequest,
            OrderEvent::CancelConfirm,
            OrderEvent::Expire,
        ];
        for event in &events {
            assert!(
                apply_event(OrderState::Rejected, event).is_err(),
                "Rejected must reject {:?}",
                event
            );
        }
    }

    #[test]
    fn test_expired_rejects_all_events() {
        let events: Vec<OrderEvent> = vec![
            OrderEvent::Submit,
            OrderEvent::Acknowledge { exchange_order_id: "x".into() },
            OrderEvent::Reject { reason: "r".into() },
            OrderEvent::PartialFill { filled_qty: Decimal::from(1) },
            OrderEvent::Fill { filled_qty: Decimal::from(1) },
            OrderEvent::CancelRequest,
            OrderEvent::CancelConfirm,
            OrderEvent::Expire,
        ];
        for event in &events {
            assert!(
                apply_event(OrderState::Expired, event).is_err(),
                "Expired must reject {:?}",
                event
            );
        }
    }

    // ======================================================================
    // Invalid transitions (wrong event for non-terminal state)
    // ======================================================================

    #[test]
    fn test_pending_rejects_cancel_request() {
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
    fn test_pending_rejects_cancel_confirm() {
        assert!(apply_event(OrderState::Pending, &OrderEvent::CancelConfirm).is_err());
    }

    #[test]
    fn test_pending_rejects_expire() {
        assert!(apply_event(OrderState::Pending, &OrderEvent::Expire).is_err());
    }

    #[test]
    fn test_submitted_rejects_cancel_request() {
        // Can't cancel what hasn't been acknowledged yet
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

    #[test]
    fn test_acknowledged_rejects_cancel_confirm() {
        // Must go through PendingCancel first
        assert!(apply_event(OrderState::Acknowledged, &OrderEvent::CancelConfirm).is_err());
    }

    // ======================================================================
    // Multi-step lifecycle sequences
    // ======================================================================

    #[test]
    fn test_lifecycle_happy_path_full_fill() {
        // Pending → Submitted → Acknowledged → Filled
        let s = apply_event(OrderState::Pending, &OrderEvent::Submit).unwrap();
        assert_eq!(s, OrderState::Submitted);
        let s = apply_event(s, &OrderEvent::Acknowledge { exchange_order_id: "e1".into() }).unwrap();
        assert_eq!(s, OrderState::Acknowledged);
        let s = apply_event(s, &OrderEvent::Fill { filled_qty: Decimal::from(10) }).unwrap();
        assert_eq!(s, OrderState::Filled);
    }

    #[test]
    fn test_lifecycle_partial_then_full_fill() {
        // Pending → Submitted → Acknowledged → PartiallyFilled → PartiallyFilled → Filled
        let s = apply_event(OrderState::Pending, &OrderEvent::Submit).unwrap();
        let s = apply_event(s, &OrderEvent::Acknowledge { exchange_order_id: "e1".into() }).unwrap();
        let s = apply_event(s, &OrderEvent::PartialFill { filled_qty: Decimal::from(3) }).unwrap();
        assert_eq!(s, OrderState::PartiallyFilled);
        let s = apply_event(s, &OrderEvent::PartialFill { filled_qty: Decimal::from(4) }).unwrap();
        assert_eq!(s, OrderState::PartiallyFilled);
        let s = apply_event(s, &OrderEvent::Fill { filled_qty: Decimal::from(3) }).unwrap();
        assert_eq!(s, OrderState::Filled);
    }

    #[test]
    fn test_lifecycle_cancel_path() {
        // Pending → Submitted → Acknowledged → PendingCancel → Cancelled
        let s = apply_event(OrderState::Pending, &OrderEvent::Submit).unwrap();
        let s = apply_event(s, &OrderEvent::Acknowledge { exchange_order_id: "e1".into() }).unwrap();
        let s = apply_event(s, &OrderEvent::CancelRequest).unwrap();
        assert_eq!(s, OrderState::PendingCancel);
        let s = apply_event(s, &OrderEvent::CancelConfirm).unwrap();
        assert_eq!(s, OrderState::Cancelled);
    }

    #[test]
    fn test_lifecycle_cancel_with_partial_fill_then_full_fill() {
        // Acknowledged → PartiallyFilled → PendingCancel → (partial fill) → (full fill wins)
        let s = OrderState::Acknowledged;
        let s = apply_event(s, &OrderEvent::PartialFill { filled_qty: Decimal::from(3) }).unwrap();
        let s = apply_event(s, &OrderEvent::CancelRequest).unwrap();
        assert_eq!(s, OrderState::PendingCancel);
        let s = apply_event(s, &OrderEvent::PartialFill { filled_qty: Decimal::from(2) }).unwrap();
        assert_eq!(s, OrderState::PendingCancel); // cancel intent preserved
        let s = apply_event(s, &OrderEvent::Fill { filled_qty: Decimal::from(5) }).unwrap();
        assert_eq!(s, OrderState::Filled); // fill wins the race
    }

    #[test]
    fn test_lifecycle_fill_before_ack() {
        // Pending → Submitted → Filled (exchange filled before ack arrived)
        let s = apply_event(OrderState::Pending, &OrderEvent::Submit).unwrap();
        let s = apply_event(s, &OrderEvent::Fill { filled_qty: Decimal::from(10) }).unwrap();
        assert_eq!(s, OrderState::Filled);
    }

    #[test]
    fn test_lifecycle_fill_while_pending() {
        // Pending → Filled (extreme out-of-order: fill before we even submitted)
        let s = apply_event(OrderState::Pending, &OrderEvent::Fill { filled_qty: Decimal::from(10) }).unwrap();
        assert_eq!(s, OrderState::Filled);
    }

    #[test]
    fn test_lifecycle_partial_fill_before_ack() {
        // Pending → Submitted → PartiallyFilled → Filled
        let s = apply_event(OrderState::Pending, &OrderEvent::Submit).unwrap();
        let s = apply_event(s, &OrderEvent::PartialFill { filled_qty: Decimal::from(3) }).unwrap();
        assert_eq!(s, OrderState::PartiallyFilled);
        let s = apply_event(s, &OrderEvent::Fill { filled_qty: Decimal::from(7) }).unwrap();
        assert_eq!(s, OrderState::Filled);
    }

    #[test]
    fn test_lifecycle_reject_from_pending() {
        // Pending → Rejected (risk check failure)
        let s = apply_event(
            OrderState::Pending,
            &OrderEvent::Reject { reason: "risk check failed".into() },
        ).unwrap();
        assert_eq!(s, OrderState::Rejected);
    }

    #[test]
    fn test_lifecycle_submitted_expire() {
        // Submitted → Expired (exchange expired before ack)
        let s = apply_event(OrderState::Submitted, &OrderEvent::Expire).unwrap();
        assert_eq!(s, OrderState::Expired);
    }

    // ======================================================================
    // State properties
    // ======================================================================

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
        assert!(!OrderState::PendingAmend.is_terminal());
        assert!(!OrderState::PendingDecrease.is_terminal());
    }

    #[test]
    fn test_open_states() {
        assert!(OrderState::Pending.is_open());
        assert!(OrderState::Submitted.is_open());
        assert!(OrderState::Acknowledged.is_open());
        assert!(OrderState::PartiallyFilled.is_open());
        assert!(OrderState::PendingCancel.is_open());
        assert!(OrderState::PendingAmend.is_open());
        assert!(OrderState::PendingDecrease.is_open());

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

    // ======================================================================
    // resolve_exchange_state tests
    // ======================================================================

    #[test]
    fn test_resolve_submitted_resting() {
        let result = resolve_exchange_state(
            &OrderState::Submitted,
            &crate::types::ExchangeOrderState::Resting,
        );
        assert_eq!(result, Some(OrderState::Acknowledged));
    }

    #[test]
    fn test_resolve_submitted_executed() {
        let result = resolve_exchange_state(
            &OrderState::Submitted,
            &crate::types::ExchangeOrderState::Executed,
        );
        assert_eq!(result, Some(OrderState::Filled));
    }

    #[test]
    fn test_resolve_submitted_not_found() {
        let result = resolve_exchange_state(
            &OrderState::Submitted,
            &crate::types::ExchangeOrderState::NotFound,
        );
        assert_eq!(result, Some(OrderState::Rejected));
    }

    #[test]
    fn test_resolve_submitted_cancelled() {
        let result = resolve_exchange_state(
            &OrderState::Submitted,
            &crate::types::ExchangeOrderState::Cancelled,
        );
        assert_eq!(result, Some(OrderState::Cancelled));
    }

    #[test]
    fn test_resolve_pending_cancel_cancelled() {
        let result = resolve_exchange_state(
            &OrderState::PendingCancel,
            &crate::types::ExchangeOrderState::Cancelled,
        );
        assert_eq!(result, Some(OrderState::Cancelled));
    }

    #[test]
    fn test_resolve_pending_cancel_executed() {
        let result = resolve_exchange_state(
            &OrderState::PendingCancel,
            &crate::types::ExchangeOrderState::Executed,
        );
        assert_eq!(result, Some(OrderState::Filled));
    }

    #[test]
    fn test_resolve_pending_cancel_resting_needs_special_handling() {
        // PendingCancel + Resting means cancel hasn't been processed yet — needs re-send
        let result = resolve_exchange_state(
            &OrderState::PendingCancel,
            &crate::types::ExchangeOrderState::Resting,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_unknown_combination_returns_none() {
        // Acknowledged + NotFound is not in the resolution table
        let result = resolve_exchange_state(
            &OrderState::Acknowledged,
            &crate::types::ExchangeOrderState::NotFound,
        );
        assert_eq!(result, None);
    }

    // ======================================================================
    // Amend transitions
    // ======================================================================

    #[test]
    fn test_acknowledged_to_pending_amend() {
        let result = apply_event(OrderState::Acknowledged, &OrderEvent::AmendRequest);
        assert_eq!(result.unwrap(), OrderState::PendingAmend);
    }

    #[test]
    fn test_partially_filled_to_pending_amend() {
        let result = apply_event(OrderState::PartiallyFilled, &OrderEvent::AmendRequest);
        assert_eq!(result.unwrap(), OrderState::PendingAmend);
    }

    #[test]
    fn test_pending_amend_to_acknowledged_on_confirm() {
        let result = apply_event(OrderState::PendingAmend, &OrderEvent::AmendConfirm);
        assert_eq!(result.unwrap(), OrderState::Acknowledged);
    }

    #[test]
    fn test_pending_amend_fill_wins() {
        let result = apply_event(
            OrderState::PendingAmend,
            &OrderEvent::Fill { filled_qty: Decimal::from(10) },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_pending_amend_partial_fill_preserves_amend() {
        let result = apply_event(
            OrderState::PendingAmend,
            &OrderEvent::PartialFill { filled_qty: Decimal::from(3) },
        );
        assert_eq!(result.unwrap(), OrderState::PendingAmend);
    }

    #[test]
    fn test_pending_amend_cancel_request() {
        let result = apply_event(OrderState::PendingAmend, &OrderEvent::CancelRequest);
        assert_eq!(result.unwrap(), OrderState::PendingCancel);
    }

    #[test]
    fn test_pending_rejects_amend_request() {
        assert!(apply_event(OrderState::Pending, &OrderEvent::AmendRequest).is_err());
    }

    #[test]
    fn test_submitted_rejects_amend_request() {
        assert!(apply_event(OrderState::Submitted, &OrderEvent::AmendRequest).is_err());
    }

    // ======================================================================
    // Decrease transitions
    // ======================================================================

    #[test]
    fn test_acknowledged_to_pending_decrease() {
        let result = apply_event(OrderState::Acknowledged, &OrderEvent::DecreaseRequest);
        assert_eq!(result.unwrap(), OrderState::PendingDecrease);
    }

    #[test]
    fn test_partially_filled_to_pending_decrease() {
        let result = apply_event(OrderState::PartiallyFilled, &OrderEvent::DecreaseRequest);
        assert_eq!(result.unwrap(), OrderState::PendingDecrease);
    }

    #[test]
    fn test_pending_decrease_to_acknowledged_on_confirm() {
        let result = apply_event(OrderState::PendingDecrease, &OrderEvent::DecreaseConfirm);
        assert_eq!(result.unwrap(), OrderState::Acknowledged);
    }

    #[test]
    fn test_pending_decrease_fill_wins() {
        let result = apply_event(
            OrderState::PendingDecrease,
            &OrderEvent::Fill { filled_qty: Decimal::from(10) },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_pending_decrease_partial_fill_preserves_decrease() {
        let result = apply_event(
            OrderState::PendingDecrease,
            &OrderEvent::PartialFill { filled_qty: Decimal::from(3) },
        );
        assert_eq!(result.unwrap(), OrderState::PendingDecrease);
    }

    #[test]
    fn test_pending_decrease_cancel_request() {
        let result = apply_event(OrderState::PendingDecrease, &OrderEvent::CancelRequest);
        assert_eq!(result.unwrap(), OrderState::PendingCancel);
    }

    #[test]
    fn test_pending_rejects_decrease_request() {
        assert!(apply_event(OrderState::Pending, &OrderEvent::DecreaseRequest).is_err());
    }

    #[test]
    fn test_submitted_rejects_decrease_request() {
        assert!(apply_event(OrderState::Submitted, &OrderEvent::DecreaseRequest).is_err());
    }

    // ======================================================================
    // Fill acceptance from new states (exhaustive update)
    // ======================================================================

    #[test]
    fn test_fill_accepted_from_pending_amend() {
        let result = apply_event(
            OrderState::PendingAmend,
            &OrderEvent::Fill { filled_qty: Decimal::from(10) },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    #[test]
    fn test_fill_accepted_from_pending_decrease() {
        let result = apply_event(
            OrderState::PendingDecrease,
            &OrderEvent::Fill { filled_qty: Decimal::from(10) },
        );
        assert_eq!(result.unwrap(), OrderState::Filled);
    }

    // ======================================================================
    // State properties for new states
    // ======================================================================

    #[test]
    fn test_pending_amend_is_open_not_terminal() {
        assert!(OrderState::PendingAmend.is_open());
        assert!(!OrderState::PendingAmend.is_terminal());
    }

    #[test]
    fn test_pending_decrease_is_open_not_terminal() {
        assert!(OrderState::PendingDecrease.is_open());
        assert!(!OrderState::PendingDecrease.is_terminal());
    }

    #[test]
    fn test_pending_amend_display() {
        assert_eq!(OrderState::PendingAmend.to_string(), "pending_amend");
    }

    #[test]
    fn test_pending_decrease_display() {
        assert_eq!(OrderState::PendingDecrease.to_string(), "pending_decrease");
    }

    // ======================================================================
    // Lifecycle: amend path
    // ======================================================================

    #[test]
    fn test_lifecycle_amend_happy_path() {
        let s = OrderState::Acknowledged;
        let s = apply_event(s, &OrderEvent::AmendRequest).unwrap();
        assert_eq!(s, OrderState::PendingAmend);
        let s = apply_event(s, &OrderEvent::AmendConfirm).unwrap();
        assert_eq!(s, OrderState::Acknowledged);
    }

    #[test]
    fn test_lifecycle_decrease_happy_path() {
        let s = OrderState::Acknowledged;
        let s = apply_event(s, &OrderEvent::DecreaseRequest).unwrap();
        assert_eq!(s, OrderState::PendingDecrease);
        let s = apply_event(s, &OrderEvent::DecreaseConfirm).unwrap();
        assert_eq!(s, OrderState::Acknowledged);
    }

    #[test]
    fn test_lifecycle_amend_then_cancel() {
        let s = OrderState::PendingAmend;
        let s = apply_event(s, &OrderEvent::CancelRequest).unwrap();
        assert_eq!(s, OrderState::PendingCancel);
        let s = apply_event(s, &OrderEvent::CancelConfirm).unwrap();
        assert_eq!(s, OrderState::Cancelled);
    }

    // ======================================================================
    // resolve_exchange_state for new states
    // ======================================================================

    #[test]
    fn test_resolve_pending_amend_resting() {
        let result = resolve_exchange_state(
            &OrderState::PendingAmend,
            &crate::types::ExchangeOrderState::Resting,
        );
        assert_eq!(result, Some(OrderState::Acknowledged));
    }

    #[test]
    fn test_resolve_pending_amend_executed() {
        let result = resolve_exchange_state(
            &OrderState::PendingAmend,
            &crate::types::ExchangeOrderState::Executed,
        );
        assert_eq!(result, Some(OrderState::Filled));
    }

    #[test]
    fn test_resolve_pending_amend_cancelled() {
        let result = resolve_exchange_state(
            &OrderState::PendingAmend,
            &crate::types::ExchangeOrderState::Cancelled,
        );
        assert_eq!(result, Some(OrderState::Cancelled));
    }

    #[test]
    fn test_resolve_pending_decrease_resting() {
        let result = resolve_exchange_state(
            &OrderState::PendingDecrease,
            &crate::types::ExchangeOrderState::Resting,
        );
        assert_eq!(result, Some(OrderState::Acknowledged));
    }

    #[test]
    fn test_resolve_pending_decrease_executed() {
        let result = resolve_exchange_state(
            &OrderState::PendingDecrease,
            &crate::types::ExchangeOrderState::Executed,
        );
        assert_eq!(result, Some(OrderState::Filled));
    }

    #[test]
    fn test_resolve_pending_decrease_cancelled() {
        let result = resolve_exchange_state(
            &OrderState::PendingDecrease,
            &crate::types::ExchangeOrderState::Cancelled,
        );
        assert_eq!(result, Some(OrderState::Cancelled));
    }
}
