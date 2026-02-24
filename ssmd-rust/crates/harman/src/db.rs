use chrono::{DateTime, Utc};
use deadpool_postgres::{Config, Pool, Runtime};
use rust_decimal::Decimal;
use tokio_postgres::NoTls;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::EnqueueError;
use crate::risk::{RiskLimits, RiskState};
use crate::state::OrderState;
use crate::types::{
    Action, CancelReason, Order, OrderRequest, Side, TimeInForce,
};

/// Create a connection pool from a database URL
pub fn create_pool(database_url: &str) -> Result<Pool, String> {
    // Parse the URL into deadpool config
    let pg_config: tokio_postgres::Config = database_url
        .parse()
        .map_err(|e: tokio_postgres::Error| format!("invalid database URL: {}", e))?;

    let mut cfg = Config::new();
    if let Some(host) = pg_config.get_hosts().first() {
        match host {
            tokio_postgres::config::Host::Tcp(h) => cfg.host = Some(h.clone()),
            #[cfg(unix)]
            tokio_postgres::config::Host::Unix(p) => {
                cfg.host = Some(p.to_string_lossy().to_string())
            }
        }
    }
    if let Some(port) = pg_config.get_ports().first() {
        cfg.port = Some(*port);
    }
    if let Some(user) = pg_config.get_user() {
        cfg.user = Some(user.to_string());
    }
    if let Some(password) = pg_config.get_password() {
        cfg.password = Some(String::from_utf8_lossy(password).to_string());
    }
    if let Some(dbname) = pg_config.get_dbname() {
        cfg.dbname = Some(dbname.to_string());
    }

    cfg.create_pool(Some(Runtime::Tokio1), NoTls)
        .map_err(|e| format!("failed to create pool: {}", e))
}

/// Run database migrations
pub async fn run_migrations(pool: &Pool) -> Result<(), String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("failed to get connection: {}", e))?;

    let migration_sql = include_str!("../migrations/001_initial.sql");

    client
        .batch_execute(migration_sql)
        .await
        .map_err(|e| format!("migration failed: {}", e))?;

    info!("database migrations applied successfully");
    Ok(())
}

/// The core transactional enqueue operation.
///
/// Single transaction: SELECT FOR UPDATE (risk state) → risk check → INSERT order → INSERT queue → COMMIT
pub async fn enqueue_order(
    pool: &Pool,
    request: &OrderRequest,
    session_id: i64,
    limits: &RiskLimits,
) -> Result<Order, EnqueueError> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| EnqueueError::Database(format!("pool error: {}", e)))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| EnqueueError::Database(format!("begin tx: {}", e)))?;

    // Lock and compute risk state from open orders
    // SELECT FOR UPDATE locks the rows so concurrent enqueues serialize
    // Uses (quantity - filled_quantity) to account for partially-filled orders
    let risk_rows = tx
        .query(
            "SELECT COALESCE(SUM((price_cents::NUMERIC / 100) * (quantity - filled_quantity)), 0) as open_notional \
             FROM orders \
             WHERE session_id = $1 AND state IN ('pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel') \
             FOR UPDATE",
            &[&session_id],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("risk query: {}", e)))?;

    let open_notional: Decimal = risk_rows
        .first()
        .map(|r| r.get::<_, Decimal>("open_notional"))
        .unwrap_or(Decimal::ZERO);

    let risk_state = RiskState { open_notional };

    // Risk check
    risk_state
        .check_order(request, limits)
        .map_err(EnqueueError::RiskCheck)?;

    // Insert order
    let row = tx
        .query_one(
            "INSERT INTO orders (session_id, client_order_id, ticker, side, action, quantity, price_cents, time_in_force, state) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending') \
             RETURNING id, created_at, updated_at",
            &[
                &session_id,
                &request.client_order_id,
                &request.ticker,
                &request.side.to_string(),
                &request.action.to_string(),
                &request.quantity,
                &request.price_cents,
                &request.time_in_force.to_string(),
            ],
        )
        .await
        .map_err(|e| {
            // Check for unique constraint violation (duplicate client_order_id)
            if let Some(db_err) = e.as_db_error() {
                if db_err.code() == &tokio_postgres::error::SqlState::UNIQUE_VIOLATION {
                    return EnqueueError::DuplicateClientOrderId(request.client_order_id);
                }
            }
            EnqueueError::Database(format!("insert order: {}", e))
        })?;

    let order_id: i64 = row.get("id");
    let created_at: DateTime<Utc> = row.get("created_at");
    let updated_at: DateTime<Utc> = row.get("updated_at");

    // Insert into order queue
    tx.execute(
        "INSERT INTO order_queue (order_id, action) VALUES ($1, 'submit')",
        &[&order_id],
    )
    .await
    .map_err(|e| EnqueueError::Database(format!("insert queue: {}", e)))?;

    // Insert audit log
    tx.execute(
        "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) VALUES ($1, 'none', 'pending', 'created', 'api')",
        &[&order_id],
    )
    .await
    .map_err(|e| EnqueueError::Database(format!("insert audit: {}", e)))?;

    tx.commit()
        .await
        .map_err(|e| EnqueueError::Database(format!("commit: {}", e)))?;

    debug!(order_id, client_order_id = %request.client_order_id, "order enqueued");

    Ok(Order {
        id: order_id,
        session_id,
        client_order_id: request.client_order_id,
        exchange_order_id: None,
        ticker: request.ticker.clone(),
        side: request.side,
        action: request.action,
        quantity: request.quantity,
        price_cents: request.price_cents,
        filled_quantity: 0,
        time_in_force: request.time_in_force,
        state: OrderState::Pending,
        cancel_reason: None,
        created_at,
        updated_at,
    })
}

/// Queued order item for the sweeper
#[derive(Debug)]
pub struct QueueItem {
    pub queue_id: i64,
    pub order_id: i64,
    pub action: String,
    pub order: Order,
}

/// Dequeue the next order for processing.
///
/// Uses SELECT FOR UPDATE SKIP LOCKED for concurrent sweeper safety.
/// Marks the queue item as processing, then transitions the order to submitted state.
pub async fn dequeue_order(pool: &Pool) -> Result<Option<QueueItem>, String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Dequeue with SKIP LOCKED
    let row = tx
        .query_opt(
            "SELECT q.id as queue_id, q.order_id, q.action, \
                    o.id, o.session_id, o.client_order_id, o.exchange_order_id, \
                    o.ticker, o.side, o.action as order_action, o.quantity, o.price_cents, \
                    o.filled_quantity, o.time_in_force, o.state, o.cancel_reason, \
                    o.created_at, o.updated_at \
             FROM order_queue q \
             JOIN orders o ON o.id = q.order_id \
             WHERE NOT q.processing \
             ORDER BY q.id \
             LIMIT 1 \
             FOR UPDATE OF q SKIP LOCKED",
            &[],
        )
        .await
        .map_err(|e| format!("dequeue query: {}", e))?;

    let row = match row {
        Some(r) => r,
        None => return Ok(None),
    };

    let queue_id: i64 = row.get("queue_id");
    let order_id: i64 = row.get("order_id");
    let action: String = row.get("action");

    // Mark as processing
    tx.execute(
        "UPDATE order_queue SET processing = TRUE WHERE id = $1",
        &[&queue_id],
    )
    .await
    .map_err(|e| format!("mark processing: {}", e))?;

    // Update order state to submitted (for submit actions)
    if action == "submit" {
        let from_state: String = row.get("state");
        tx.execute(
            "UPDATE orders SET state = 'submitted' WHERE id = $1",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("update state: {}", e))?;

        tx.execute(
            "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) VALUES ($1, $2, 'submitted', 'submit', 'sweeper')",
            &[&order_id, &from_state],
        )
        .await
        .map_err(|e| format!("insert audit: {}", e))?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;

    let order = Order {
        id: order_id,
        session_id: row.get("session_id"),
        client_order_id: row.get("client_order_id"),
        exchange_order_id: row.get("exchange_order_id"),
        ticker: row.get("ticker"),
        side: parse_side(row.get("side")),
        action: parse_action(row.get("order_action")),
        quantity: row.get("quantity"),
        price_cents: row.get("price_cents"),
        filled_quantity: row.get("filled_quantity"),
        time_in_force: parse_tif(row.get("time_in_force")),
        state: if action == "submit" {
            OrderState::Submitted
        } else {
            parse_state(row.get("state"))
        },
        cancel_reason: row
            .get::<_, Option<String>>("cancel_reason")
            .map(|s| parse_cancel_reason(&s)),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    };

    Ok(Some(QueueItem {
        queue_id,
        order_id,
        action,
        order,
    }))
}

/// Update an order's state after exchange interaction.
///
/// Wraps the read + update + audit in a single transaction to prevent
/// race conditions between concurrent state updates.
pub async fn update_order_state(
    pool: &Pool,
    order_id: i64,
    new_state: OrderState,
    exchange_order_id: Option<&str>,
    filled_quantity: Option<i32>,
    cancel_reason: Option<&CancelReason>,
    actor: &str,
) -> Result<(), String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Lock the order row and get current state for audit
    let row = tx
        .query_one(
            "SELECT state FROM orders WHERE id = $1 FOR UPDATE",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("get state: {}", e))?;
    let from_state: String = row.get("state");

    let state_str = new_state.to_string();
    let cancel_str = cancel_reason.map(|r| match r {
        CancelReason::UserRequested => "user_requested",
        CancelReason::RiskLimitBreached => "risk_limit_breached",
        CancelReason::Shutdown => "shutdown",
        CancelReason::Expired => "expired",
        CancelReason::ExchangeCancel => "exchange_cancel",
    });

    tx.execute(
        "UPDATE orders SET state = $1, exchange_order_id = COALESCE($2, exchange_order_id), \
         filled_quantity = COALESCE($3, filled_quantity), cancel_reason = COALESCE($4, cancel_reason) \
         WHERE id = $5",
        &[
            &state_str,
            &exchange_order_id,
            &filled_quantity,
            &cancel_str,
            &order_id,
        ],
    )
    .await
    .map_err(|e| format!("update order: {}", e))?;

    // Audit log
    tx.execute(
        "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) VALUES ($1, $2, $3, $4, $5)",
        &[&order_id, &from_state, &state_str, &state_str, &actor],
    )
    .await
    .map_err(|e| format!("insert audit: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;

    Ok(())
}

/// Remove a processed queue item
pub async fn remove_queue_item(pool: &Pool, queue_id: i64) -> Result<(), String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    client
        .execute("DELETE FROM order_queue WHERE id = $1", &[&queue_id])
        .await
        .map_err(|e| format!("delete queue item: {}", e))?;

    Ok(())
}

/// Requeue a queue item (set processing back to false)
pub async fn requeue_item(pool: &Pool, queue_id: i64) -> Result<(), String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    client
        .execute(
            "UPDATE order_queue SET processing = FALSE WHERE id = $1",
            &[&queue_id],
        )
        .await
        .map_err(|e| format!("requeue: {}", e))?;

    Ok(())
}

/// Record a fill (trade execution)
///
/// trade_id UNIQUE constraint handles deduplication.
/// Returns Ok(true) if inserted, Ok(false) if duplicate.
pub async fn record_fill(
    pool: &Pool,
    order_id: i64,
    trade_id: &str,
    price_cents: i32,
    quantity: i32,
    is_taker: bool,
    filled_at: DateTime<Utc>,
) -> Result<bool, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let result = client
        .execute(
            "INSERT INTO fills (order_id, trade_id, price_cents, quantity, is_taker, filled_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (trade_id) DO NOTHING",
            &[&order_id, &trade_id, &price_cents, &quantity, &is_taker, &filled_at],
        )
        .await
        .map_err(|e| format!("insert fill: {}", e))?;

    Ok(result > 0)
}

/// Find orders in ambiguous states (for recovery)
pub async fn get_ambiguous_orders(pool: &Pool) -> Result<Vec<Order>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT id, session_id, client_order_id, exchange_order_id, \
                    ticker, side, action, quantity, price_cents, \
                    filled_quantity, time_in_force, state, cancel_reason, \
                    created_at, updated_at \
             FROM orders \
             WHERE state IN ('submitted', 'pending_cancel') \
             ORDER BY id",
            &[],
        )
        .await
        .map_err(|e| format!("query ambiguous: {}", e))?;

    Ok(rows.iter().map(row_to_order).collect())
}

/// Get an order by ID
pub async fn get_order(pool: &Pool, order_id: i64) -> Result<Option<Order>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_opt(
            "SELECT id, session_id, client_order_id, exchange_order_id, \
                    ticker, side, action, quantity, price_cents, \
                    filled_quantity, time_in_force, state, cancel_reason, \
                    created_at, updated_at \
             FROM orders WHERE id = $1",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("get order: {}", e))?;

    Ok(row.as_ref().map(row_to_order))
}

/// Get an order by client_order_id
pub async fn get_order_by_client_id(
    pool: &Pool,
    client_order_id: Uuid,
) -> Result<Option<Order>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_opt(
            "SELECT id, session_id, client_order_id, exchange_order_id, \
                    ticker, side, action, quantity, price_cents, \
                    filled_quantity, time_in_force, state, cancel_reason, \
                    created_at, updated_at \
             FROM orders WHERE client_order_id = $1",
            &[&client_order_id],
        )
        .await
        .map_err(|e| format!("get order by cid: {}", e))?;

    Ok(row.as_ref().map(row_to_order))
}

/// List orders with optional state filter
pub async fn list_orders(
    pool: &Pool,
    state_filter: Option<OrderState>,
) -> Result<Vec<Order>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = if let Some(state) = state_filter {
        client
            .query(
                "SELECT id, session_id, client_order_id, exchange_order_id, \
                        ticker, side, action, quantity, price_cents, \
                        filled_quantity, time_in_force, state, cancel_reason, \
                        created_at, updated_at \
                 FROM orders WHERE state = $1 ORDER BY id",
                &[&state.to_string()],
            )
            .await
    } else {
        client
            .query(
                "SELECT id, session_id, client_order_id, exchange_order_id, \
                        ticker, side, action, quantity, price_cents, \
                        filled_quantity, time_in_force, state, cancel_reason, \
                        created_at, updated_at \
                 FROM orders ORDER BY id",
                &[],
            )
            .await
    }
    .map_err(|e| format!("list orders: {}", e))?;

    Ok(rows.iter().map(row_to_order).collect())
}

/// Compute risk state from database
pub async fn compute_risk_state(pool: &Pool, session_id: i64) -> Result<RiskState, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "SELECT COALESCE(SUM((price_cents::NUMERIC / 100) * (quantity - filled_quantity)), 0) as open_notional \
             FROM orders \
             WHERE session_id = $1 AND state IN ('pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel')",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("compute risk: {}", e))?;

    Ok(RiskState {
        open_notional: row.get("open_notional"),
    })
}

/// Enqueue a cancel action for an order
pub async fn enqueue_cancel(pool: &Pool, order_id: i64) -> Result<(), String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    client
        .execute(
            "INSERT INTO order_queue (order_id, action) VALUES ($1, 'cancel')",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("enqueue cancel: {}", e))?;

    Ok(())
}

/// Atomically cancel an order: lock row, verify cancellable state, update
/// state to PendingCancel, and enqueue cancel — all in one transaction.
///
/// Returns Ok(()) on success, Err with reason if the order cannot be cancelled.
pub async fn atomic_cancel_order(
    pool: &Pool,
    order_id: i64,
    cancel_reason: &CancelReason,
) -> Result<(), String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Lock the order row and get current state
    let row = tx
        .query_opt(
            "SELECT state FROM orders WHERE id = $1 FOR UPDATE",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("get order: {}", e))?;

    let row = row.ok_or_else(|| "order not found".to_string())?;
    let current_state_str: String = row.get("state");
    let current_state = parse_state(&current_state_str);

    // Only cancel if in cancellable state
    if !matches!(
        current_state,
        OrderState::Acknowledged | OrderState::PartiallyFilled
    ) {
        return Err(format!(
            "cannot cancel order in {} state",
            current_state
        ));
    }

    let cancel_str = match cancel_reason {
        CancelReason::UserRequested => "user_requested",
        CancelReason::RiskLimitBreached => "risk_limit_breached",
        CancelReason::Shutdown => "shutdown",
        CancelReason::Expired => "expired",
        CancelReason::ExchangeCancel => "exchange_cancel",
    };

    // Update state to PendingCancel
    tx.execute(
        "UPDATE orders SET state = 'pending_cancel', cancel_reason = $1 WHERE id = $2",
        &[&cancel_str, &order_id],
    )
    .await
    .map_err(|e| format!("update state: {}", e))?;

    // Enqueue cancel
    tx.execute(
        "INSERT INTO order_queue (order_id, action) VALUES ($1, 'cancel')",
        &[&order_id],
    )
    .await
    .map_err(|e| format!("enqueue cancel: {}", e))?;

    // Audit log
    tx.execute(
        "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) VALUES ($1, $2, 'pending_cancel', 'cancel_request', 'api')",
        &[&order_id, &current_state_str],
    )
    .await
    .map_err(|e| format!("insert audit: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;

    debug!(order_id, "order cancel enqueued atomically");

    Ok(())
}

/// Drain queue items during shutdown without transitioning orders through Submitted.
///
/// Directly deletes queue items and marks their orders as Rejected.
pub async fn drain_queue_for_shutdown(pool: &Pool) -> Result<u64, String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Get all pending queue items
    let rows = tx
        .query(
            "DELETE FROM order_queue RETURNING order_id",
            &[],
        )
        .await
        .map_err(|e| format!("drain queue: {}", e))?;

    let count = rows.len() as u64;

    // Mark non-terminal orders as rejected
    for row in &rows {
        let order_id: i64 = row.get("order_id");
        tx.execute(
            "UPDATE orders SET state = 'rejected', cancel_reason = 'shutdown' \
             WHERE id = $1 AND state NOT IN ('filled', 'cancelled', 'rejected', 'expired')",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("reject order: {}", e))?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;

    Ok(count)
}

// --- Helper parsers ---

fn row_to_order(row: &tokio_postgres::Row) -> Order {
    Order {
        id: row.get("id"),
        session_id: row.get("session_id"),
        client_order_id: row.get("client_order_id"),
        exchange_order_id: row.get("exchange_order_id"),
        ticker: row.get("ticker"),
        side: parse_side(row.get("side")),
        action: parse_action(row.get("action")),
        quantity: row.get("quantity"),
        price_cents: row.get("price_cents"),
        filled_quantity: row.get("filled_quantity"),
        time_in_force: parse_tif(row.get("time_in_force")),
        state: parse_state(row.get("state")),
        cancel_reason: row
            .get::<_, Option<String>>("cancel_reason")
            .map(|s| parse_cancel_reason(&s)),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn parse_side(s: &str) -> Side {
    match s {
        "yes" => Side::Yes,
        "no" => Side::No,
        _ => {
            warn!(value = s, "unknown side in DB, defaulting to Yes");
            Side::Yes
        }
    }
}

fn parse_action(s: &str) -> Action {
    match s {
        "buy" => Action::Buy,
        "sell" => Action::Sell,
        _ => {
            warn!(value = s, "unknown action in DB, defaulting to Buy");
            Action::Buy
        }
    }
}

fn parse_tif(s: &str) -> TimeInForce {
    match s {
        "gtc" => TimeInForce::Gtc,
        "ioc" => TimeInForce::Ioc,
        _ => {
            warn!(value = s, "unknown time_in_force in DB, defaulting to Gtc");
            TimeInForce::Gtc
        }
    }
}

fn parse_state(s: &str) -> OrderState {
    match s {
        "pending" => OrderState::Pending,
        "submitted" => OrderState::Submitted,
        "acknowledged" => OrderState::Acknowledged,
        "partially_filled" => OrderState::PartiallyFilled,
        "filled" => OrderState::Filled,
        "pending_cancel" => OrderState::PendingCancel,
        "cancelled" => OrderState::Cancelled,
        "rejected" => OrderState::Rejected,
        "expired" => OrderState::Expired,
        _ => {
            warn!(state = s, "unknown order state in DB, defaulting to Pending");
            OrderState::Pending
        }
    }
}

fn parse_cancel_reason(s: &str) -> CancelReason {
    match s {
        "user_requested" => CancelReason::UserRequested,
        "risk_limit_breached" => CancelReason::RiskLimitBreached,
        "shutdown" => CancelReason::Shutdown,
        "expired" => CancelReason::Expired,
        "exchange_cancel" => CancelReason::ExchangeCancel,
        _ => {
            warn!(value = s, "unknown cancel_reason in DB, defaulting to ExchangeCancel");
            CancelReason::ExchangeCancel
        }
    }
}
