use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::error::ExchangeError;
use crate::state::OrderState;
use crate::types::{
    Action, AmendRequest, AmendResult, Balance, ExchangeFill, ExchangeOrder,
    ExchangeOrderStatus, ExchangeSettlement, MarketResult, OrderRequest, Position, Side,
};

/// Trait for exchange adapters.
///
/// Implementations must handle authentication, rate limiting, and
/// mapping between our internal types and exchange-specific formats.
#[async_trait]
pub trait ExchangeAdapter: Send + Sync {
    /// Submit a new order to the exchange.
    ///
    /// Returns the exchange-assigned order ID on success.
    async fn submit_order(&self, order: &OrderRequest) -> Result<String, ExchangeError>;

    /// Cancel an order by its exchange-assigned ID.
    async fn cancel_order(&self, exchange_order_id: &str) -> Result<(), ExchangeError>;

    /// Cancel all open orders.
    ///
    /// Returns the number of orders cancelled.
    async fn cancel_all_orders(&self) -> Result<i32, ExchangeError>;

    /// Get order status by our client_order_id (idempotency key).
    ///
    /// Used during recovery to resolve ambiguous order states when
    /// exchange_order_id is not available (e.g., Submitted orders where
    /// the POST response was lost).
    async fn get_order_by_client_id(
        &self,
        client_order_id: Uuid,
    ) -> Result<ExchangeOrderStatus, ExchangeError>;

    /// Get order status by the exchange-assigned order ID.
    ///
    /// Preferred over `get_order_by_client_id` when the exchange order ID
    /// is known. Uses the direct single-order endpoint which is more reliable
    /// than list-based lookups.
    async fn get_order_by_exchange_id(
        &self,
        exchange_order_id: &str,
    ) -> Result<ExchangeOrderStatus, ExchangeError>;

    /// Get current portfolio positions.
    async fn get_positions(&self) -> Result<Vec<Position>, ExchangeError>;

    /// Get resting (open) orders from the exchange.
    ///
    /// Returns orders in resting state. Used by reconciliation to discover
    /// external orders placed outside of harman (e.g., via exchange website).
    async fn get_orders(&self) -> Result<Vec<ExchangeOrder>, ExchangeError>;

    /// Get fills (trade executions), optionally filtered by minimum timestamp.
    ///
    /// When `min_ts` is Some, only returns fills at or after the given time.
    /// Implementations must paginate internally to return all matching fills.
    async fn get_fills(&self, min_ts: Option<DateTime<Utc>>) -> Result<Vec<ExchangeFill>, ExchangeError>;

    /// Get current account balance.
    async fn get_balance(&self) -> Result<Balance, ExchangeError>;

    /// Amend a resting order's price and/or quantity.
    /// Loses queue priority on the exchange.
    async fn amend_order(&self, request: &AmendRequest) -> Result<AmendResult, ExchangeError>;

    /// Decrease a resting order's quantity (preserves queue priority).
    async fn decrease_order(
        &self,
        exchange_order_id: &str,
        reduce_by: Decimal,
    ) -> Result<(), ExchangeError>;

    /// Check if a market is still active (accepting orders).
    ///
    /// Returns `true` if the market is open/active, `false` if closed/settled/finalized.
    /// Used as a fallback when portfolio-level queries can't determine order state
    /// (e.g., single-order GET returns 404 on demo for settled markets).
    async fn is_market_active(&self, ticker: &str) -> Result<bool, ExchangeError>;

    /// Get settlements (market close payouts), optionally filtered by minimum timestamp
    /// and/or ticker.
    ///
    /// When `min_ts` is Some, only returns settlements at or after the given time.
    /// When `ticker` is Some, only returns settlements for that specific ticker.
    /// Implementations must paginate internally to return all matching settlements.
    async fn get_settlements(
        &self,
        min_ts: Option<DateTime<Utc>>,
        ticker: Option<&str>,
    ) -> Result<Vec<ExchangeSettlement>, ExchangeError>;
}

// --- WebSocket event types ---

/// Events delivered by the exchange via WebSocket (or other real-time channels).
///
/// WS is read-only — order placement/cancel/amend remains REST.
/// These events are produced by `EventStream` and consumed by the `EventIngester`.
#[derive(Debug, Clone)]
pub enum ExchangeEvent {
    /// An order's state changed on the exchange.
    OrderUpdate {
        exchange_order_id: String,
        client_order_id: Option<Uuid>,
        ticker: String,
        status: OrderState,
        filled_quantity: Decimal,
        remaining_quantity: Decimal,
        /// Number of contracts cancelled due to market close (Kalshi-specific).
        close_cancel_count: Option<i64>,
    },
    /// A fill (trade execution) occurred.
    Fill {
        trade_id: String,
        exchange_order_id: String,
        ticker: String,
        side: Side,
        action: Action,
        price_dollars: Decimal,
        quantity: Decimal,
        is_taker: bool,
        filled_at: DateTime<Utc>,
        client_order_id: Option<Uuid>,
    },
    /// Portfolio position update for a ticker.
    PositionUpdate {
        ticker: String,
        position: Decimal,
        position_cost_dollars: Decimal,
        realized_pnl_dollars: Decimal,
        volume: Decimal,
    },
    /// A market has settled (determined/finalized).
    MarketSettled {
        ticker: String,
        result: MarketResult,
        settled_time: DateTime<Utc>,
    },
    /// WebSocket connection established (or re-established after disconnect).
    Connected,
    /// WebSocket connection lost.
    Disconnected { reason: String },
}

/// Trait for exchange event streams (WebSocket private channels).
///
/// Implementations are pure event sources — no DB access, no business logic.
/// The `EventIngester` consumes these events and routes them to shared processors.
pub trait EventStream: Send + Sync {
    /// Subscribe to exchange events. Returns a broadcast receiver.
    ///
    /// Multiple consumers can subscribe; each gets all events.
    /// The broadcast channel has a bounded buffer — slow consumers will lag.
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ExchangeEvent>;
}
