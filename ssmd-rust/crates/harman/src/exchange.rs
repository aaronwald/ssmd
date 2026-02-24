use async_trait::async_trait;
use uuid::Uuid;

use crate::error::ExchangeError;
use crate::types::{
    Balance, ExchangeFill, ExchangeOrderStatus, OrderRequest, Position,
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
    /// Used during recovery to resolve ambiguous order states.
    async fn get_order_by_client_id(
        &self,
        client_order_id: Uuid,
    ) -> Result<ExchangeOrderStatus, ExchangeError>;

    /// Get current portfolio positions.
    async fn get_positions(&self) -> Result<Vec<Position>, ExchangeError>;

    /// Get recent fills (trade executions).
    ///
    /// Returns fills ordered by time, most recent first.
    async fn get_fills(&self) -> Result<Vec<ExchangeFill>, ExchangeError>;

    /// Get current account balance.
    async fn get_balance(&self) -> Result<Balance, ExchangeError>;
}
