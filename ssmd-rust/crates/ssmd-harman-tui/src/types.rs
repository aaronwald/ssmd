use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

/// Order state enum matching the harman state machine.
/// Deserialized from the API's snake_case string representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    Pending,
    Submitted,
    Acknowledged,
    PartiallyFilled,
    Filled,
    PendingCancel,
    PendingAmend,
    PendingDecrease,
    Cancelled,
    Rejected,
    Expired,
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

impl OrderState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            OrderState::Filled
                | OrderState::Cancelled
                | OrderState::Rejected
                | OrderState::Expired
        )
    }
}

/// Side of an order (yes/no for prediction markets)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeInForce {
    Gtc,
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

/// Cancel reason
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    UserRequested,
    RiskLimitBreached,
    Shutdown,
    Expired,
    ExchangeCancel,
}

/// Order as returned by the harman API.
/// Fields serialized as strings by the server are deserialized accordingly.
#[derive(Debug, Clone, Deserialize)]
pub struct Order {
    pub id: i64,
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

/// Response wrapper for GET /v1/orders
#[derive(Debug, Deserialize)]
pub struct OrdersResponse {
    pub orders: Vec<Order>,
}

/// Response from GET /v1/admin/risk.
/// All decimal values are returned as strings by the API.
#[derive(Debug, Deserialize)]
pub struct RiskInfo {
    #[serde(with = "rust_decimal::serde::str")]
    pub max_notional: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub open_notional: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub available_notional: Decimal,
}

/// Response from POST /v1/admin/pump
#[derive(Debug, Deserialize)]
pub struct PumpResult {
    pub processed: u64,
    pub submitted: u64,
    pub rejected: u64,
    pub cancelled: u64,
    #[serde(default)]
    pub amended: u64,
    #[serde(default)]
    pub decreased: u64,
    pub requeued: u64,
    pub errors: Vec<String>,
}

/// Response from POST /v1/admin/reconcile
#[derive(Debug, Deserialize)]
pub struct ReconcileResult {
    pub fills_discovered: u64,
    pub orders_resolved: u64,
    pub errors: Vec<String>,
}

/// Response from POST /v1/orders/mass-cancel
#[derive(Debug, Deserialize)]
pub struct MassCancelResult {
    pub cancelled: u64,
}

/// Response from GET /health
#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    pub status: String,
}

/// Request body for POST /v1/orders
#[derive(Debug, serde::Serialize)]
pub struct CreateOrderRequest {
    pub client_order_id: Uuid,
    pub ticker: String,
    pub side: String,
    pub action: String,
    pub quantity: String,
    pub price_dollars: String,
    pub time_in_force: String,
}

/// Request body for POST /v1/orders/:id/amend
#[derive(Debug, serde::Serialize)]
pub struct AmendOrderRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_price_dollars: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_quantity: Option<String>,
}

/// Request body for POST /v1/orders/:id/decrease
#[derive(Debug, serde::Serialize)]
pub struct DecreaseOrderRequest {
    pub reduce_by: String,
}

/// Raw response from data-ts GET /v1/data/snap.
/// Snapshots are raw JSON values — structure varies by feed.
#[derive(Debug, Deserialize)]
pub struct SnapResponse {
    pub feed: String,
    pub snapshots: Vec<serde_json::Value>,
    pub count: usize,
}

/// Normalized snapshot for TUI display. Built from raw JSON per-feed.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub ticker: String,
    // Kalshi fields (dollars)
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub no_bid: Option<f64>,
    pub no_ask: Option<f64>,
    pub last_price: Option<f64>,
    // Kraken fields
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub last: Option<f64>,
    pub funding_rate: Option<f64>,
    // Polymarket fields
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub spread: Option<f64>,
}

impl Snapshot {
    /// Parse a Kalshi snapshot. Price data is nested under `msg`.
    pub fn from_kalshi(v: &serde_json::Value) -> Option<Self> {
        let ticker = v.get("_ticker")?.as_str()?.to_string();
        let msg = v.get("msg")?;
        let yes_bid = Self::parse_dollar_str(msg, "yes_bid_dollars")
            .or_else(|| msg.get("yes_bid").and_then(|v| v.as_f64()).map(|c| c / 100.0));
        let yes_ask = Self::parse_dollar_str(msg, "yes_ask_dollars")
            .or_else(|| msg.get("yes_ask").and_then(|v| v.as_f64()).map(|c| c / 100.0));
        let last_price = Self::parse_dollar_str(msg, "price_dollars")
            .or_else(|| msg.get("price").and_then(|v| v.as_f64()).map(|c| c / 100.0));
        // no_bid/no_ask are 1 - yes_ask / 1 - yes_bid for binary markets
        let no_bid = yes_ask.map(|a| 1.0 - a);
        let no_ask = yes_bid.map(|b| 1.0 - b);
        Some(Self {
            ticker,
            yes_bid,
            yes_ask,
            no_bid,
            no_ask,
            last_price,
            bid: None,
            ask: None,
            last: None,
            funding_rate: None,
            best_bid: None,
            best_ask: None,
            spread: None,
        })
    }

    /// Parse a Kraken snapshot. Fields are flat at the top level.
    pub fn from_kraken(v: &serde_json::Value) -> Option<Self> {
        let ticker = v.get("_ticker")?.as_str()?.to_string();
        Some(Self {
            ticker,
            bid: v.get("bid").and_then(|v| v.as_f64()),
            ask: v.get("ask").and_then(|v| v.as_f64()),
            last: v.get("last").and_then(|v| v.as_f64()),
            funding_rate: v.get("funding_rate").and_then(|v| v.as_f64()),
            yes_bid: None,
            yes_ask: None,
            no_bid: None,
            no_ask: None,
            last_price: None,
            best_bid: None,
            best_ask: None,
            spread: None,
        })
    }

    /// Parse a Polymarket snapshot. Uses first outcome in price_changes.
    pub fn from_polymarket(v: &serde_json::Value) -> Option<Self> {
        let ticker = v.get("_ticker")?.as_str()?.to_string();
        let changes = v.get("price_changes")?.as_array()?;
        let first = changes.first()?;
        let best_bid = first.get("best_bid").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok());
        let best_ask = first.get("best_ask").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok());
        let spread = match (best_bid, best_ask) {
            (Some(b), Some(a)) => Some(a - b),
            _ => None,
        };
        Some(Self {
            ticker,
            best_bid,
            best_ask,
            spread,
            yes_bid: None,
            yes_ask: None,
            no_bid: None,
            no_ask: None,
            last_price: None,
            bid: None,
            ask: None,
            last: None,
            funding_rate: None,
        })
    }

    fn parse_dollar_str(msg: &serde_json::Value, field: &str) -> Option<f64> {
        msg.get(field)?.as_str()?.parse::<f64>().ok()
    }
}
