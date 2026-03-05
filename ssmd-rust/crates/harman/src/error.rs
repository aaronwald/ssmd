use thiserror::Error;
use uuid::Uuid;

use crate::state::OrderState;

/// Errors from state machine transitions
#[derive(Error, Debug)]
pub enum TransitionError {
    #[error("invalid transition from {from:?} via {event}: {reason}")]
    InvalidTransition {
        from: OrderState,
        event: String,
        reason: String,
    },
}

/// Errors from risk checks
#[derive(Error, Debug)]
pub enum RiskCheckError {
    #[error("max notional exceeded: current={current}, requested={requested}, limit={limit}")]
    MaxNotionalExceeded {
        current: rust_decimal::Decimal,
        requested: rust_decimal::Decimal,
        limit: rust_decimal::Decimal,
    },

    #[error("max order notional exceeded: order_notional={order_notional}, limit={limit}")]
    MaxOrderNotionalExceeded {
        order_notional: rust_decimal::Decimal,
        limit: rust_decimal::Decimal,
    },

    #[error("daily loss limit exceeded: daily_pnl={daily_pnl}, limit={limit}")]
    DailyLossExceeded {
        daily_pnl: rust_decimal::Decimal,
        limit: rust_decimal::Decimal,
    },
}

/// Errors from order enqueue operations
#[derive(Error, Debug)]
pub enum EnqueueError {
    #[error("duplicate client_order_id: {0}")]
    DuplicateClientOrderId(Uuid),

    #[error("risk check failed: {0}")]
    RiskCheck(#[from] RiskCheckError),

    #[error("database error: {0}")]
    Database(String),
}

/// Errors from exchange operations
#[derive(Error, Debug)]
pub enum ExchangeError {
    #[error("order rejected by exchange: {reason}")]
    Rejected { reason: String },

    #[error("rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("order not found by client_order_id: {0}")]
    OrderNotFoundByClientId(Uuid),

    #[error("order not found by exchange_order_id: {0}")]
    OrderNotFoundByExchangeId(String),

    #[error("exchange timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("exchange connection error: {0}")]
    Connection(String),

    #[error("exchange returned unexpected response: {0}")]
    Unexpected(String),

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("WebSocket disconnected: {reason}")]
    Disconnected { reason: String },

    #[error("WebSocket subscription failed: channel={channel}, reason={reason}")]
    SubscriptionFailed { channel: String, reason: String },
}

impl ExchangeError {
    /// Returns true if this is any variant of order-not-found.
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            ExchangeError::OrderNotFoundByClientId(_)
                | ExchangeError::OrderNotFoundByExchangeId(_)
        )
    }
}
