//! Test helpers for harman crash recovery testing.
//!
//! Provides a `MockExchange` implementing `ExchangeAdapter` with configurable
//! behaviors, and DB test utilities for setting up and asserting order state.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::ExchangeError;
use crate::exchange::ExchangeAdapter;
use crate::types::{
    Action, AmendRequest, AmendResult, Balance, ExchangeFill, ExchangeOrderState,
    ExchangeOrderStatus, OrderRequest, Position, Side,
};

/// Configurable response for `submit_order`.
#[derive(Clone, Debug)]
pub enum SubmitBehavior {
    /// Return Ok with an auto-generated exchange order ID.
    Accept,
    /// Return Ok with a specific exchange order ID.
    AcceptWithId(String),
    /// Return Err(Rejected).
    Reject(String),
    /// Return Err(Timeout).
    Timeout,
    /// Return Err(RateLimited).
    RateLimited(u64),
}

/// Configurable response for `cancel_order`.
#[derive(Clone, Debug)]
pub enum CancelBehavior {
    /// Return Ok(()).
    Accept,
    /// Return Err(NotFound).
    NotFound,
}

/// Configurable response for `amend_order`.
#[derive(Clone, Debug)]
pub enum AmendBehavior {
    /// Return Ok with default AmendResult.
    Accept,
    /// Return Err(Rejected).
    Reject(String),
    /// Return Err(NotFound).
    NotFound,
}

/// Configurable response for `decrease_order`.
#[derive(Clone, Debug)]
pub enum DecreaseBehavior {
    /// Return Ok(()).
    Accept,
    /// Return Err(Rejected).
    Reject(String),
    /// Return Err(NotFound).
    NotFound,
}

/// Internal state for MockExchange, protected by a Mutex.
#[derive(Debug)]
pub struct MockExchangeState {
    /// Default behavior for submit_order.
    pub submit_behavior: SubmitBehavior,
    /// Per-client_order_id overrides for submit behavior.
    pub submit_overrides: HashMap<Uuid, SubmitBehavior>,
    /// Default behavior for cancel_order.
    pub cancel_behavior: CancelBehavior,
    /// How many orders cancel_all_orders returns.
    pub cancel_all_count: i32,
    /// Exchange order statuses, keyed by client_order_id.
    pub order_statuses: HashMap<Uuid, ExchangeOrderStatus>,
    /// Fills to return from get_fills.
    pub fills: Vec<ExchangeFill>,
    /// Positions to return from get_positions.
    pub positions: Vec<Position>,
    /// Balance to return from get_balance.
    pub balance: Balance,
    /// Auto-incrementing exchange order ID counter.
    next_id: u64,
    /// Log of submitted orders (for assertions).
    pub submitted_orders: Vec<OrderRequest>,
    /// Log of cancel calls (exchange_order_id).
    pub cancel_calls: Vec<String>,
    /// How many times cancel_all_orders was called.
    pub cancel_all_calls: u64,
    /// Default behavior for amend_order.
    pub amend_behavior: AmendBehavior,
    /// Log of amend calls.
    pub amend_calls: Vec<AmendRequest>,
    /// Default behavior for decrease_order.
    pub decrease_behavior: DecreaseBehavior,
    /// Log of decrease calls (exchange_order_id, reduce_by).
    pub decrease_calls: Vec<(String, Decimal)>,
}

impl Default for MockExchangeState {
    fn default() -> Self {
        Self {
            submit_behavior: SubmitBehavior::Accept,
            submit_overrides: HashMap::new(),
            cancel_behavior: CancelBehavior::Accept,
            cancel_all_count: 0,
            order_statuses: HashMap::new(),
            fills: Vec::new(),
            positions: Vec::new(),
            balance: Balance {
                available_dollars: Decimal::new(10000, 2),
                total_dollars: Decimal::new(10000, 2),
            },
            next_id: 1,
            submitted_orders: Vec::new(),
            cancel_calls: Vec::new(),
            cancel_all_calls: 0,
            amend_behavior: AmendBehavior::Accept,
            amend_calls: Vec::new(),
            decrease_behavior: DecreaseBehavior::Accept,
            decrease_calls: Vec::new(),
        }
    }
}

impl MockExchangeState {
    fn next_exchange_id(&mut self) -> String {
        let id = format!("mock-exch-{}", self.next_id);
        self.next_id += 1;
        id
    }
}

/// A mock exchange adapter for testing.
///
/// Thread-safe via `Arc<Mutex<MockExchangeState>>`. Tests can modify
/// the inner state to configure exchange responses mid-test.
pub struct MockExchange {
    pub state: Arc<Mutex<MockExchangeState>>,
}

impl Default for MockExchange {
    fn default() -> Self {
        Self::new()
    }
}

impl MockExchange {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockExchangeState::default())),
        }
    }

    pub fn with_state(state: Arc<Mutex<MockExchangeState>>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl ExchangeAdapter for MockExchange {
    async fn submit_order(&self, order: &OrderRequest) -> Result<String, ExchangeError> {
        let mut state = self.state.lock().await;
        state.submitted_orders.push(order.clone());

        let behavior = state
            .submit_overrides
            .get(&order.client_order_id)
            .cloned()
            .unwrap_or_else(|| state.submit_behavior.clone());

        match behavior {
            SubmitBehavior::Accept => {
                let id = state.next_exchange_id();
                Ok(id)
            }
            SubmitBehavior::AcceptWithId(id) => Ok(id),
            SubmitBehavior::Reject(reason) => Err(ExchangeError::Rejected { reason }),
            SubmitBehavior::Timeout => Err(ExchangeError::Timeout { timeout_ms: 5000 }),
            SubmitBehavior::RateLimited(ms) => {
                Err(ExchangeError::RateLimited { retry_after_ms: ms })
            }
        }
    }

    async fn cancel_order(&self, exchange_order_id: &str) -> Result<(), ExchangeError> {
        let mut state = self.state.lock().await;
        state.cancel_calls.push(exchange_order_id.to_string());

        match &state.cancel_behavior {
            CancelBehavior::Accept => Ok(()),
            CancelBehavior::NotFound => Err(ExchangeError::NotFound(Uuid::nil())),
        }
    }

    async fn cancel_all_orders(&self) -> Result<i32, ExchangeError> {
        let mut state = self.state.lock().await;
        state.cancel_all_calls += 1;
        Ok(state.cancel_all_count)
    }

    async fn get_order_by_client_id(
        &self,
        client_order_id: Uuid,
    ) -> Result<ExchangeOrderStatus, ExchangeError> {
        let state = self.state.lock().await;
        state
            .order_statuses
            .get(&client_order_id)
            .cloned()
            .ok_or(ExchangeError::NotFound(client_order_id))
    }

    async fn get_positions(&self) -> Result<Vec<Position>, ExchangeError> {
        let state = self.state.lock().await;
        Ok(state.positions.clone())
    }

    async fn get_fills(&self, _min_ts: Option<chrono::DateTime<chrono::Utc>>) -> Result<Vec<ExchangeFill>, ExchangeError> {
        let state = self.state.lock().await;
        Ok(state.fills.clone())
    }

    async fn get_balance(&self) -> Result<Balance, ExchangeError> {
        let state = self.state.lock().await;
        Ok(state.balance.clone())
    }

    async fn amend_order(&self, request: &AmendRequest) -> Result<AmendResult, ExchangeError> {
        let mut state = self.state.lock().await;
        state.amend_calls.push(request.clone());

        match &state.amend_behavior {
            AmendBehavior::Accept => Ok(AmendResult {
                exchange_order_id: request.exchange_order_id.clone(),
                new_price_dollars: request.new_price_dollars.unwrap_or(Decimal::ZERO),
                new_quantity: request.new_quantity.unwrap_or(Decimal::ZERO),
                filled_quantity: Decimal::ZERO,
                remaining_quantity: request.new_quantity.unwrap_or(Decimal::ZERO),
            }),
            AmendBehavior::Reject(reason) => Err(ExchangeError::Rejected {
                reason: reason.clone(),
            }),
            AmendBehavior::NotFound => Err(ExchangeError::NotFound(Uuid::nil())),
        }
    }

    async fn decrease_order(
        &self,
        exchange_order_id: &str,
        reduce_by: Decimal,
    ) -> Result<(), ExchangeError> {
        let mut state = self.state.lock().await;
        state
            .decrease_calls
            .push((exchange_order_id.to_string(), reduce_by));

        match &state.decrease_behavior {
            DecreaseBehavior::Accept => Ok(()),
            DecreaseBehavior::Reject(reason) => Err(ExchangeError::Rejected {
                reason: reason.clone(),
            }),
            DecreaseBehavior::NotFound => Err(ExchangeError::NotFound(Uuid::nil())),
        }
    }
}

// =============================================================================
// DB test utilities (require a real PostgreSQL connection via DATABASE_URL)
// =============================================================================

use crate::db;
use crate::state::OrderState;
use deadpool_postgres::Pool;

/// Insert an order directly into the DB in a specific state.
///
/// Bypasses the normal enqueue flow to set up test scenarios.
pub async fn insert_test_order(
    pool: &Pool,
    session_id: i64,
    state: OrderState,
    ticker: &str,
    exchange_order_id: Option<&str>,
) -> Result<i64, String> {
    let client_order_id = Uuid::new_v4();
    insert_test_order_with_coid(pool, session_id, state, ticker, exchange_order_id, client_order_id)
        .await
}

/// Insert an order with a specific client_order_id.
pub async fn insert_test_order_with_coid(
    pool: &Pool,
    session_id: i64,
    state: OrderState,
    ticker: &str,
    exchange_order_id: Option<&str>,
    client_order_id: Uuid,
) -> Result<i64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "INSERT INTO prediction_orders \
             (session_id, client_order_id, exchange_order_id, ticker, side, action, \
              quantity, price_dollars, filled_quantity, time_in_force, state) \
             VALUES ($1, $2, $3, $4, 'yes', 'buy', 10, 0.50, 0, 'gtc', $5) \
             RETURNING id",
            &[
                &session_id,
                &client_order_id,
                &exchange_order_id,
                &ticker,
                &state.to_string(),
            ],
        )
        .await
        .map_err(|e| format!("insert test order: {}", e))?;

    Ok(row.get("id"))
}

/// Insert a queue item for an order.
pub async fn insert_test_queue_item(
    pool: &Pool,
    order_id: i64,
    action: &str,
) -> Result<i64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "INSERT INTO order_queue (order_id, action, actor) VALUES ($1, $2, 'test') RETURNING id",
            &[&order_id, &action],
        )
        .await
        .map_err(|e| format!("insert test queue item: {}", e))?;

    Ok(row.get("id"))
}

/// Insert a queue item with metadata for an order (used for amend/decrease tests).
pub async fn insert_test_queue_item_with_metadata(
    pool: &Pool,
    order_id: i64,
    action: &str,
    metadata: serde_json::Value,
) -> Result<i64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "INSERT INTO order_queue (order_id, action, actor, metadata) VALUES ($1, $2, 'test', $3) RETURNING id",
            &[&order_id, &action, &metadata],
        )
        .await
        .map_err(|e| format!("insert test queue item with metadata: {}", e))?;

    Ok(row.get("id"))
}

/// Assert that an order is in the expected state.
pub async fn assert_order_state(
    pool: &Pool,
    order_id: i64,
    expected_state: OrderState,
) -> Result<(), String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "SELECT state FROM prediction_orders WHERE id = $1",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("query order state: {}", e))?;

    let actual: String = row.get("state");
    let expected_str = expected_state.to_string();

    if actual != expected_str {
        return Err(format!(
            "order {} state mismatch: expected={}, actual={}",
            order_id, expected_str, actual
        ));
    }

    Ok(())
}

/// Get an order's price and quantity (for amend/decrease assertions).
pub async fn get_order_price_qty(
    pool: &Pool,
    order_id: i64,
) -> Result<(Decimal, Decimal), String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "SELECT price_dollars, quantity FROM prediction_orders WHERE id = $1",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("get order price/qty: {}", e))?;

    Ok((row.get("price_dollars"), row.get("quantity")))
}

/// Get the number of items in the order queue for a session.
pub async fn queue_count(pool: &Pool, session_id: i64) -> Result<i64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "SELECT COUNT(*) as cnt FROM order_queue q \
             JOIN prediction_orders o ON o.id = q.order_id \
             WHERE o.session_id = $1",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("count queue: {}", e))?;

    Ok(row.get("cnt"))
}

/// Create a test pool and run migrations. Requires DATABASE_URL env var.
pub async fn setup_test_db() -> Result<Pool, String> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL not set".to_string())?;
    let pool = db::create_pool(&url)?;
    db::run_migrations(&pool).await?;
    Ok(pool)
}

/// Create a test session, returning its ID.
pub async fn create_test_session(pool: &Pool) -> Result<i64, String> {
    db::get_or_create_session(pool, "test", None)
        .await
}

/// Helper to build an ExchangeOrderStatus for mock configuration.
pub fn mock_exchange_status(
    exchange_order_id: &str,
    status: ExchangeOrderState,
    filled_qty: Decimal,
    remaining_qty: Decimal,
) -> ExchangeOrderStatus {
    ExchangeOrderStatus {
        exchange_order_id: exchange_order_id.to_string(),
        status,
        filled_quantity: filled_qty,
        remaining_quantity: remaining_qty,
    }
}

/// Helper to build a mock ExchangeFill.
pub fn mock_fill(
    order_id: &str,
    ticker: &str,
    quantity: Decimal,
    price: Decimal,
) -> ExchangeFill {
    ExchangeFill {
        trade_id: format!("test-trade-{}", Uuid::new_v4()),
        order_id: order_id.to_string(),
        ticker: ticker.to_string(),
        side: Side::Yes,
        action: Action::Buy,
        price_dollars: price,
        quantity,
        is_taker: true,
        filled_at: Utc::now(),
        client_order_id: None,
    }
}
