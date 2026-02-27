use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::OrderState;

/// Side of an order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Yes,
    No,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Side::Yes => write!(f, "yes"),
            Side::No => write!(f, "no"),
        }
    }
}

/// Order action type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Buy,
    Sell,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Buy => write!(f, "buy"),
            Action::Sell => write!(f, "sell"),
        }
    }
}

/// Time-in-force for orders
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeInForce {
    /// Good-til-cancelled
    Gtc,
    /// Immediate-or-cancel
    Ioc,
}

impl std::fmt::Display for TimeInForce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeInForce::Gtc => write!(f, "gtc"),
            TimeInForce::Ioc => write!(f, "ioc"),
        }
    }
}

impl TimeInForce {
    /// Returns the Kalshi API string representation.
    pub fn to_kalshi_str(&self) -> &'static str {
        match self {
            TimeInForce::Gtc => "good_till_canceled",
            TimeInForce::Ioc => "immediate_or_cancel",
        }
    }
}

/// Reason for order cancellation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    UserRequested,
    RiskLimitBreached,
    Shutdown,
    Expired,
    ExchangeCancel,
}

/// Request to create an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub client_order_id: Uuid,
    pub ticker: String,
    pub side: Side,
    pub action: Action,
    #[serde(with = "rust_decimal::serde::str")]
    pub quantity: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub price_dollars: Decimal,
    pub time_in_force: TimeInForce,
}

impl OrderRequest {
    /// Compute the notional value of this order in dollars
    pub fn notional(&self) -> Decimal {
        self.price_dollars * self.quantity
    }
}

/// An order in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: i64,
    pub session_id: i64,
    pub client_order_id: Uuid,
    pub exchange_order_id: Option<String>,
    pub ticker: String,
    pub side: Side,
    pub action: Action,
    #[serde(with = "rust_decimal::serde::str")]
    pub quantity: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub price_dollars: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub filled_quantity: Decimal,
    pub time_in_force: TimeInForce,
    pub state: OrderState,
    pub cancel_reason: Option<CancelReason>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A fill (trade execution) for an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub id: i64,
    pub order_id: i64,
    pub trade_id: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub price_dollars: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub quantity: Decimal,
    pub is_taker: bool,
    pub filled_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Exchange-side order status for reconciliation
#[derive(Debug, Clone)]
pub struct ExchangeOrderStatus {
    pub exchange_order_id: String,
    pub status: ExchangeOrderState,
    pub filled_quantity: Decimal,
    pub remaining_quantity: Decimal,
}

/// Simplified exchange order state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeOrderState {
    Resting,
    Executed,
    Cancelled,
    NotFound,
}

/// Portfolio position from exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub ticker: String,
    pub side: Side,
    #[serde(with = "rust_decimal::serde::str")]
    pub quantity: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub market_value_dollars: Decimal,
}

/// Portfolio balance from exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    #[serde(with = "rust_decimal::serde::str")]
    pub available_dollars: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub total_dollars: Decimal,
}

/// Request to amend a resting order.
/// Caller provides original ticker/side/action (required by Kalshi).
/// At least one of new_price_dollars or new_quantity must be Some.
#[derive(Debug, Clone)]
pub struct AmendRequest {
    pub exchange_order_id: String,
    pub ticker: String,
    pub side: Side,
    pub action: Action,
    pub new_price_dollars: Option<Decimal>,
    pub new_quantity: Option<Decimal>,
}

/// Result of an order amendment
#[derive(Debug, Clone)]
pub struct AmendResult {
    pub exchange_order_id: String,
    pub new_price_dollars: Decimal,
    pub new_quantity: Decimal,
    pub filled_quantity: Decimal,
    pub remaining_quantity: Decimal,
}

/// Exchange fill record
#[derive(Debug, Clone)]
pub struct ExchangeFill {
    pub trade_id: String,
    pub order_id: String,
    pub ticker: String,
    pub side: Side,
    pub action: Action,
    pub price_dollars: Decimal,
    pub quantity: Decimal,
    pub is_taker: bool,
    pub filled_at: DateTime<Utc>,
    /// None for external fills (placed on exchange website, not via harman)
    pub client_order_id: Option<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_request_notional() {
        let req = OrderRequest {
            client_order_id: Uuid::new_v4(),
            ticker: "KXBTCD-26FEB-T100000".to_string(),
            side: Side::Yes,
            action: Action::Buy,
            quantity: Decimal::from(10),
            price_dollars: Decimal::new(50, 2),
            time_in_force: TimeInForce::Gtc,
        };
        // 10 contracts at $0.50 each = $5.00
        assert_eq!(req.notional(), Decimal::new(500, 2));
    }

    #[test]
    fn test_order_request_notional_high_price() {
        let req = OrderRequest {
            client_order_id: Uuid::new_v4(),
            ticker: "KXBTCD-26FEB-T100000".to_string(),
            side: Side::Yes,
            action: Action::Buy,
            quantity: Decimal::from(100),
            price_dollars: Decimal::new(99, 2),
            time_in_force: TimeInForce::Gtc,
        };
        // 100 contracts at $0.99 each = $99.00
        assert_eq!(req.notional(), Decimal::new(9900, 2));
    }

    #[test]
    fn test_side_display() {
        assert_eq!(Side::Yes.to_string(), "yes");
        assert_eq!(Side::No.to_string(), "no");
    }

    #[test]
    fn test_action_display() {
        assert_eq!(Action::Buy.to_string(), "buy");
        assert_eq!(Action::Sell.to_string(), "sell");
    }

    #[test]
    fn test_time_in_force_display() {
        assert_eq!(TimeInForce::Gtc.to_string(), "gtc");
        assert_eq!(TimeInForce::Ioc.to_string(), "ioc");
    }

    #[test]
    fn test_time_in_force_kalshi_str() {
        assert_eq!(TimeInForce::Gtc.to_kalshi_str(), "good_till_canceled");
        assert_eq!(TimeInForce::Ioc.to_kalshi_str(), "immediate_or_cancel");
    }
}
