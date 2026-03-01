use chrono::{DateTime, Utc};
use deadpool_postgres::{Config, Pool, Runtime};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json;
use tokio_postgres::NoTls;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::EnqueueError;
use crate::risk::{RiskLimits, RiskState};
use crate::state::OrderState;
use crate::types::{
    Action, CancelReason, GroupState, GroupType, LegRole, Order, OrderGroup, OrderRequest, Side,
    TimeInForce,
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

    // NoTls is acceptable here: harman connects to ssmd-postgres within the same
    // K8s namespace. Network policies restrict access to port 5432. For external
    // Postgres connections, replace with tokio-postgres-rustls and sslmode=require.
    cfg.create_pool(Some(Runtime::Tokio1), NoTls)
        .map_err(|e| format!("failed to create pool: {}", e))
}

/// Run database migrations
pub async fn run_migrations(pool: &Pool) -> Result<(), String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("failed to get connection: {}", e))?;

    // Always run 001 (idempotent via IF NOT EXISTS)
    let migration_001 = include_str!("../migrations/001_initial.sql");
    client
        .batch_execute(migration_001)
        .await
        .map_err(|e| format!("migration 001 failed: {}", e))?;

    // Create schema_migrations table (idempotent)
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version TEXT PRIMARY KEY,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"
        )
        .await
        .map_err(|e| format!("create schema_migrations failed: {}", e))?;

    // Check if 002 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '002_decimal_migration'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 002: {}", e))?;

    if row.is_none() {
        let migration_002 = include_str!("../migrations/002_decimal_migration.sql");
        client
            .batch_execute(migration_002)
            .await
            .map_err(|e| format!("migration 002 failed: {}", e))?;
        info!("migration 002_decimal_migration applied");
    }

    // Check if 003 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '003_amend_decrease'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 003: {}", e))?;

    if row.is_none() {
        let migration_003 = include_str!("../migrations/003_amend_decrease.sql");
        client
            .batch_execute(migration_003)
            .await
            .map_err(|e| format!("migration 003 failed: {}", e))?;
        info!("migration 003_amend_decrease applied");
    }

    // Check if 004 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '004_session_key_prefix'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 004: {}", e))?;

    if row.is_none() {
        let migration_004 = include_str!("../migrations/004_session_key_prefix.sql");
        client
            .batch_execute(migration_004)
            .await
            .map_err(|e| format!("migration 004 failed: {}", e))?;
        info!("migration 004_session_key_prefix applied");
    }

    // Check if 005 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '005_order_groups'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 005: {}", e))?;

    if row.is_none() {
        let migration_005 = include_str!("../migrations/005_order_groups.sql");
        client
            .batch_execute(migration_005)
            .await
            .map_err(|e| format!("migration 005 failed: {}", e))?;
        info!("migration 005_order_groups applied");
    }

    // Check if 006 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '006_session_environment'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 006: {}", e))?;

    if row.is_none() {
        let migration_006 = include_str!("../migrations/006_session_environment.sql");
        client
            .batch_execute(migration_006)
            .await
            .map_err(|e| format!("migration 006 failed: {}", e))?;
        info!("migration 006_session_environment applied");
    }

    // Check if 007 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '007_session_risk'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 007: {}", e))?;

    if row.is_none() {
        let migration_007 = include_str!("../migrations/007_session_risk.sql");
        client
            .batch_execute(migration_007)
            .await
            .map_err(|e| format!("migration 007 failed: {}", e))?;
        info!("migration 007_session_risk applied");
    }

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

    // Lock open order rows to serialize concurrent enqueues, then compute risk.
    // FOR UPDATE cannot be combined with aggregate functions in PostgreSQL,
    // so we lock first, then aggregate in a separate query within the same tx.
    tx.query(
        "SELECT id FROM prediction_orders \
         WHERE session_id = $1 AND state IN ('pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease') \
         FOR UPDATE",
        &[&session_id],
    )
    .await
    .map_err(|e| EnqueueError::Database(format!("risk lock: {}", e)))?;

    let risk_row = tx
        .query_one(
            "SELECT COALESCE(SUM(price_dollars * (quantity - filled_quantity)), 0) as open_notional \
             FROM prediction_orders \
             WHERE session_id = $1 AND state IN ('pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease')",
            &[&session_id],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("risk query: {}", e)))?;

    let open_notional: Decimal = risk_row.get::<_, Decimal>("open_notional");

    let risk_state = RiskState { open_notional };

    // Query per-session risk limit; fall back to global
    let session_row = tx
        .query_one(
            "SELECT max_notional FROM sessions WHERE id = $1",
            &[&session_id],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("session risk query: {}", e)))?;

    let effective_limits = match session_row.get::<_, Option<Decimal>>("max_notional") {
        Some(session_max) => RiskLimits { max_notional: session_max },
        None => limits.clone(),
    };

    // Risk check
    risk_state
        .check_order(request, &effective_limits)
        .map_err(EnqueueError::RiskCheck)?;

    // Insert order
    let row = tx
        .query_one(
            "INSERT INTO prediction_orders (session_id, client_order_id, ticker, side, action, quantity, price_dollars, time_in_force, state) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending') \
             RETURNING id, created_at, updated_at",
            &[
                &session_id,
                &request.client_order_id,
                &request.ticker,
                &request.side.to_string(),
                &request.action.to_string(),
                &request.quantity,
                &request.price_dollars,
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
        "INSERT INTO order_queue (order_id, action, actor) VALUES ($1, 'submit', 'api')",
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
        price_dollars: request.price_dollars,
        filled_quantity: Decimal::ZERO,
        time_in_force: request.time_in_force,
        state: OrderState::Pending,
        cancel_reason: None,
        group_id: None,
        leg_role: None,
        created_at,
        updated_at,
    })
}

/// Queued order item for the pump
#[derive(Debug)]
pub struct QueueItem {
    pub queue_id: i64,
    pub order_id: i64,
    pub action: String,
    pub order: Order,
    pub metadata: Option<serde_json::Value>,
}

/// Dequeue the next order for processing, scoped to a session.
///
/// Uses SELECT FOR UPDATE SKIP LOCKED for concurrent safety.
/// Marks the queue item as processing, then transitions the order to submitted state.
pub async fn dequeue_order(pool: &Pool, session_id: i64) -> Result<Option<QueueItem>, String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Dequeue with SKIP LOCKED, filtered by session
    let row = tx
        .query_opt(
            "SELECT q.id as queue_id, q.order_id, q.action, q.metadata, \
                    o.id, o.session_id, o.client_order_id, o.exchange_order_id, \
                    o.ticker, o.side, o.action as order_action, o.quantity, o.price_dollars, \
                    o.filled_quantity, o.time_in_force, o.state, o.cancel_reason, \
                    o.group_id, o.leg_role, o.created_at, o.updated_at \
             FROM order_queue q \
             JOIN prediction_orders o ON o.id = q.order_id \
             WHERE NOT q.processing AND o.session_id = $1 \
             ORDER BY q.id \
             LIMIT 1 \
             FOR UPDATE OF q SKIP LOCKED",
            &[&session_id],
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
            "UPDATE prediction_orders SET state = 'submitted' WHERE id = $1",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("update state: {}", e))?;

        tx.execute(
            "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) VALUES ($1, $2, 'submitted', 'submit', 'pump')",
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
        price_dollars: row.get("price_dollars"),
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
        group_id: row.get("group_id"),
        leg_role: row
            .get::<_, Option<String>>("leg_role")
            .map(|s| parse_leg_role(&s)),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    };

    let metadata: Option<serde_json::Value> = row.get("metadata");

    Ok(Some(QueueItem {
        queue_id,
        order_id,
        action,
        order,
        metadata,
    }))
}

/// Update an order's state after exchange interaction.
///
/// Wraps the read + update + audit in a single transaction to prevent
/// race conditions between concurrent state updates.
#[allow(clippy::too_many_arguments)]
pub async fn update_order_state(
    pool: &Pool,
    order_id: i64,
    session_id: i64,
    new_state: OrderState,
    exchange_order_id: Option<&str>,
    filled_quantity: Option<Decimal>,
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

    // Lock the order row and get current state for audit (scoped to session)
    let row = tx
        .query_one(
            "SELECT state FROM prediction_orders WHERE id = $1 AND session_id = $2 FOR UPDATE",
            &[&order_id, &session_id],
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
        "UPDATE prediction_orders SET state = $1, exchange_order_id = COALESCE($2, exchange_order_id), \
         filled_quantity = COALESCE($3, filled_quantity), cancel_reason = COALESCE($4, cancel_reason) \
         WHERE id = $5 AND session_id = $6",
        &[
            &state_str,
            &exchange_order_id,
            &filled_quantity,
            &cancel_str,
            &order_id,
            &session_id,
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

/// Record a fill (trade execution), validating the order belongs to the session.
///
/// Uses INSERT ... SELECT to atomically verify order ownership.
/// trade_id UNIQUE constraint handles deduplication.
/// Returns Ok(true) if inserted, Ok(false) if duplicate or order not in session.
#[allow(clippy::too_many_arguments)]
pub async fn record_fill(
    pool: &Pool,
    order_id: i64,
    session_id: i64,
    trade_id: &str,
    price_dollars: Decimal,
    quantity: Decimal,
    is_taker: bool,
    filled_at: DateTime<Utc>,
) -> Result<bool, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let result = client
        .execute(
            "INSERT INTO fills (order_id, trade_id, price_dollars, quantity, is_taker, filled_at) \
             SELECT $1, $2, $3, $4, $5, $6 \
             WHERE EXISTS (SELECT 1 FROM prediction_orders WHERE id = $1 AND session_id = $7) \
             ON CONFLICT (trade_id) DO NOTHING",
            &[&order_id, &trade_id, &price_dollars, &quantity, &is_taker, &filled_at, &session_id],
        )
        .await
        .map_err(|e| format!("insert fill: {}", e))?;

    Ok(result > 0)
}

/// Get or create a session for the given exchange and optional API key prefix.
///
/// Returns the ID of an open (not closed) session, or creates a new one.
/// Uses an advisory lock to prevent duplicate session creation during
/// concurrent startup (e.g., crash-loop restart race).
///
/// When `key_prefix` is None, finds/creates a session where api_key_prefix IS NULL
/// (backward compatible with pre-auth behavior). When Some, scopes to that key.
pub async fn get_or_create_session(
    pool: &Pool,
    exchange: &str,
    environment: &str,
    key_prefix: Option<&str>,
) -> Result<i64, String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Advisory lock scoped to this transaction — serializes concurrent session creation
    // for the same exchange+env+prefix. Released automatically on commit.
    let lock_key = match key_prefix {
        Some(prefix) => format!("{}:{}:{}", exchange, environment, prefix),
        None => format!("{}:{}", exchange, environment),
    };
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtext($1))",
        &[&lock_key],
    )
    .await
    .map_err(|e| format!("advisory lock: {}", e))?;

    // Look for an existing open session
    let row = match key_prefix {
        Some(prefix) => {
            tx.query_opt(
                "SELECT id FROM sessions WHERE exchange = $1 AND environment = $2 AND api_key_prefix = $3 AND closed_at IS NULL ORDER BY id DESC LIMIT 1",
                &[&exchange, &environment, &prefix],
            )
            .await
        }
        None => {
            tx.query_opt(
                "SELECT id FROM sessions WHERE exchange = $1 AND environment = $2 AND api_key_prefix IS NULL AND closed_at IS NULL ORDER BY id DESC LIMIT 1",
                &[&exchange, &environment],
            )
            .await
        }
    }
    .map_err(|e| format!("query session: {:?}", e))?;

    if let Some(row) = row {
        let id: i64 = row.get("id");
        tx.commit()
            .await
            .map_err(|e| format!("commit: {}", e))?;
        info!(session_id = id, exchange, environment, key_prefix, "using existing session");
        return Ok(id);
    }

    // Create a new session
    let row = tx
        .query_one(
            "INSERT INTO sessions (exchange, environment, api_key_prefix) VALUES ($1, $2, $3) RETURNING id",
            &[&exchange, &environment, &key_prefix],
        )
        .await
        .map_err(|e| format!("create session: {:?}", e))?;

    let id: i64 = row.get("id");
    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;
    info!(session_id = id, exchange, environment, key_prefix, "created new session");
    Ok(id)
}

/// Get total filled quantity for an order from the fills table.
///
/// Used by reconciliation to determine if an order should transition
/// to Filled or PartiallyFilled after discovering new fills.
pub async fn get_filled_quantity(pool: &Pool, order_id: i64) -> Result<Decimal, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "SELECT COALESCE(SUM(quantity), 0) as total_filled FROM fills WHERE order_id = $1",
            &[&order_id],
        )
        .await
        .map_err(|e| format!("get filled quantity: {}", e))?;

    Ok(row.get("total_filled"))
}

/// A fill record returned by list_fills.
#[derive(Debug, Serialize, Deserialize)]
pub struct Fill {
    pub id: i64,
    pub order_id: i64,
    pub ticker: String,
    pub side: String,
    pub action: String,
    pub trade_id: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub price_dollars: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub quantity: Decimal,
    pub is_taker: bool,
    pub filled_at: DateTime<Utc>,
}

/// An audit log entry returned by list_audit_log.
#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub order_id: i64,
    pub ticker: String,
    pub from_state: String,
    pub to_state: String,
    pub event: String,
    pub actor: String,
    pub created_at: DateTime<Utc>,
}

/// List fills for a session, joining with prediction_orders to get ticker/side/action.
pub async fn list_fills(pool: &Pool, session_id: i64, limit: i64) -> Result<Vec<Fill>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT f.id, f.order_id, o.ticker, o.side, o.action, f.trade_id, \
             f.price_dollars, f.quantity, f.is_taker, f.filled_at \
             FROM fills f \
             JOIN prediction_orders o ON f.order_id = o.id \
             WHERE o.session_id = $1 \
             ORDER BY f.filled_at DESC \
             LIMIT $2",
            &[&session_id, &limit],
        )
        .await
        .map_err(|e| format!("list fills: {}", e))?;

    Ok(rows
        .iter()
        .map(|row| Fill {
            id: row.get("id"),
            order_id: row.get("order_id"),
            ticker: row.get("ticker"),
            side: row.get("side"),
            action: row.get("action"),
            trade_id: row.get("trade_id"),
            price_dollars: row.get("price_dollars"),
            quantity: row.get("quantity"),
            is_taker: row.get("is_taker"),
            filled_at: row.get("filled_at"),
        })
        .collect())
}

/// List audit log entries for a session, joining with prediction_orders to get ticker.
pub async fn list_audit_log(
    pool: &Pool,
    session_id: i64,
    limit: i64,
) -> Result<Vec<AuditEntry>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT a.id, a.order_id, o.ticker, a.from_state, a.to_state, \
             a.event, a.actor, a.created_at \
             FROM audit_log a \
             JOIN prediction_orders o ON a.order_id = o.id \
             WHERE o.session_id = $1 \
             ORDER BY a.created_at DESC \
             LIMIT $2",
            &[&session_id, &limit],
        )
        .await
        .map_err(|e| format!("list audit log: {}", e))?;

    Ok(rows
        .iter()
        .map(|row| AuditEntry {
            id: row.get("id"),
            order_id: row.get("order_id"),
            ticker: row.get("ticker"),
            from_state: row.get("from_state"),
            to_state: row.get("to_state"),
            event: row.get("event"),
            actor: row.get("actor"),
            created_at: row.get("created_at"),
        })
        .collect())
}

/// Find orders in ambiguous states (for recovery and reconciliation).
///
/// Includes 'acknowledged' because orders can be filled or cancelled on the
/// exchange while still showing as acknowledged locally (e.g., fills received
/// out-of-band, mass cancel on exchange).
pub async fn get_ambiguous_orders(pool: &Pool, session_id: i64) -> Result<Vec<Order>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT id, session_id, client_order_id, exchange_order_id, \
                    ticker, side, action, quantity, price_dollars, \
                    filled_quantity, time_in_force, state, cancel_reason, \
                    group_id, leg_role, created_at, updated_at \
             FROM prediction_orders \
             WHERE session_id = $1 AND state IN ('submitted', 'acknowledged', 'pending_cancel', 'pending_amend', 'pending_decrease') \
             ORDER BY id",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("query ambiguous: {}", e))?;

    Ok(rows.iter().map(row_to_order).collect())
}

/// Get an order by ID, scoped to a session
pub async fn get_order(pool: &Pool, order_id: i64, session_id: i64) -> Result<Option<Order>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_opt(
            "SELECT id, session_id, client_order_id, exchange_order_id, \
                    ticker, side, action, quantity, price_dollars, \
                    filled_quantity, time_in_force, state, cancel_reason, \
                    group_id, leg_role, created_at, updated_at \
             FROM prediction_orders WHERE id = $1 AND session_id = $2",
            &[&order_id, &session_id],
        )
        .await
        .map_err(|e| format!("get order: {}", e))?;

    Ok(row.as_ref().map(row_to_order))
}

/// Get an order by client_order_id, scoped to a session
pub async fn get_order_by_client_id(
    pool: &Pool,
    client_order_id: Uuid,
    session_id: i64,
) -> Result<Option<Order>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_opt(
            "SELECT id, session_id, client_order_id, exchange_order_id, \
                    ticker, side, action, quantity, price_dollars, \
                    filled_quantity, time_in_force, state, cancel_reason, \
                    group_id, leg_role, created_at, updated_at \
             FROM prediction_orders WHERE client_order_id = $1 AND session_id = $2",
            &[&client_order_id, &session_id],
        )
        .await
        .map_err(|e| format!("get order by cid: {}", e))?;

    Ok(row.as_ref().map(row_to_order))
}

/// List orders with optional state filter, scoped to a session
pub async fn list_orders(
    pool: &Pool,
    session_id: i64,
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
                        ticker, side, action, quantity, price_dollars, \
                        filled_quantity, time_in_force, state, cancel_reason, \
                        group_id, leg_role, created_at, updated_at \
                 FROM prediction_orders WHERE session_id = $1 AND state = $2 ORDER BY id",
                &[&session_id, &state.to_string()],
            )
            .await
    } else {
        client
            .query(
                "SELECT id, session_id, client_order_id, exchange_order_id, \
                        ticker, side, action, quantity, price_dollars, \
                        filled_quantity, time_in_force, state, cancel_reason, \
                        group_id, leg_role, created_at, updated_at \
                 FROM prediction_orders WHERE session_id = $1 ORDER BY id",
                &[&session_id],
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
            "SELECT COALESCE(SUM(price_dollars * (quantity - filled_quantity)), 0) as open_notional \
             FROM prediction_orders \
             WHERE session_id = $1 AND state IN ('pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease')",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("compute risk: {}", e))?;

    Ok(RiskState {
        open_notional: row.get("open_notional"),
    })
}

/// Get the per-session max_notional override (NULL = use global)
pub async fn get_session_max_notional(
    pool: &Pool,
    session_id: i64,
) -> Result<Option<Decimal>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "SELECT max_notional FROM sessions WHERE id = $1",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("get session risk: {}", e))?;

    Ok(row.get("max_notional"))
}

/// Session info returned by list_sessions
#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: i64,
    pub api_key_prefix: Option<String>,
    pub display_name: Option<String>,
    pub max_notional: Option<String>,
    pub suspended: bool,
    pub open_notional: String,
    pub created_at: String,
    pub closed_at: Option<String>,
}

/// List all sessions for an exchange+environment, with open_notional for each
pub async fn list_sessions(
    pool: &Pool,
    exchange: &str,
    environment: &str,
    is_suspended: impl Fn(i64) -> bool,
) -> Result<Vec<SessionInfo>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT id, api_key_prefix, display_name, max_notional, \
                    created_at::text, closed_at::text \
             FROM sessions \
             WHERE exchange = $1 AND environment = $2 \
             ORDER BY id",
            &[&exchange, &environment],
        )
        .await
        .map_err(|e| format!("list sessions: {}", e))?;

    let mut sessions = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: i64 = row.get("id");
        let max_notional: Option<Decimal> = row.get("max_notional");
        let closed_at: Option<String> = row.get("closed_at");

        // Compute open_notional for open sessions
        let open_notional = if closed_at.is_none() {
            match compute_risk_state(pool, id).await {
                Ok(rs) => rs.open_notional,
                Err(_) => Decimal::ZERO,
            }
        } else {
            Decimal::ZERO
        };

        sessions.push(SessionInfo {
            id,
            api_key_prefix: row.get("api_key_prefix"),
            display_name: row.get("display_name"),
            max_notional: max_notional.map(|d| d.to_string()),
            suspended: is_suspended(id),
            open_notional: open_notional.to_string(),
            created_at: row.get("created_at"),
            closed_at,
        });
    }

    Ok(sessions)
}

/// Update the per-session risk limit (NULL = reset to global)
pub async fn update_session_risk(
    pool: &Pool,
    session_id: i64,
    max_notional: Option<Decimal>,
) -> Result<bool, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let count = client
        .execute(
            "UPDATE sessions SET max_notional = $2 WHERE id = $1",
            &[&session_id, &max_notional],
        )
        .await
        .map_err(|e| format!("update session risk: {}", e))?;

    Ok(count > 0)
}

/// List active (open) session IDs for an exchange+environment
pub async fn list_active_session_ids(
    pool: &Pool,
    exchange: &str,
    environment: &str,
) -> Result<Vec<i64>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT id FROM sessions WHERE exchange = $1 AND environment = $2 AND closed_at IS NULL ORDER BY id",
            &[&exchange, &environment],
        )
        .await
        .map_err(|e| format!("list active sessions: {}", e))?;

    Ok(rows.iter().map(|r| r.get("id")).collect())
}

/// Enqueue a cancel action for an order
pub async fn enqueue_cancel(pool: &Pool, order_id: i64, actor: &str) -> Result<(), String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    client
        .execute(
            "INSERT INTO order_queue (order_id, action, actor) VALUES ($1, 'cancel', $2)",
            &[&order_id, &actor],
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
    session_id: i64,
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

    // Lock the order row and get current state (scoped to session)
    let row = tx
        .query_opt(
            "SELECT state FROM prediction_orders WHERE id = $1 AND session_id = $2 FOR UPDATE",
            &[&order_id, &session_id],
        )
        .await
        .map_err(|e| format!("get order: {}", e))?;

    let row = row.ok_or_else(|| "order not found".to_string())?;
    let current_state_str: String = row.get("state");
    let current_state = parse_state(&current_state_str);

    // Only cancel if in cancellable state
    if !matches!(
        current_state,
        OrderState::Pending | OrderState::Submitted | OrderState::Acknowledged | OrderState::PartiallyFilled
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
        "UPDATE prediction_orders SET state = 'pending_cancel', cancel_reason = $1 WHERE id = $2",
        &[&cancel_str, &order_id],
    )
    .await
    .map_err(|e| format!("update state: {}", e))?;

    // Enqueue cancel
    tx.execute(
        "INSERT INTO order_queue (order_id, action, actor) VALUES ($1, 'cancel', 'api')",
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

/// Atomically amend an order: lock row, verify amendable state, update to PendingAmend,
/// enqueue amend with metadata — all in one transaction.
///
/// At least one of new_price_dollars or new_quantity must be provided.
pub async fn atomic_amend_order(
    pool: &Pool,
    order_id: i64,
    session_id: i64,
    new_price_dollars: Option<Decimal>,
    new_quantity: Option<Decimal>,
) -> Result<(), String> {
    if new_price_dollars.is_none() && new_quantity.is_none() {
        return Err("at least one of new_price_dollars or new_quantity required".to_string());
    }

    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Lock the order row and get current state (scoped to session)
    let row = tx
        .query_opt(
            "SELECT state FROM prediction_orders WHERE id = $1 AND session_id = $2 FOR UPDATE",
            &[&order_id, &session_id],
        )
        .await
        .map_err(|e| format!("get order: {}", e))?;

    let row = row.ok_or_else(|| "order not found".to_string())?;
    let current_state_str: String = row.get("state");
    let current_state = parse_state(&current_state_str);

    // Only amend if in amendable state
    if !matches!(
        current_state,
        OrderState::Acknowledged | OrderState::PartiallyFilled
    ) {
        return Err(format!(
            "cannot amend order in {} state",
            current_state
        ));
    }

    // Build metadata
    let mut metadata = serde_json::Map::new();
    if let Some(price) = new_price_dollars {
        metadata.insert("new_price_dollars".to_string(), serde_json::json!(price.to_string()));
    }
    if let Some(qty) = new_quantity {
        metadata.insert("new_quantity".to_string(), serde_json::json!(qty.to_string()));
    }
    let metadata_json = serde_json::Value::Object(metadata);

    // Update state to PendingAmend
    tx.execute(
        "UPDATE prediction_orders SET state = 'pending_amend' WHERE id = $1",
        &[&order_id],
    )
    .await
    .map_err(|e| format!("update state: {}", e))?;

    // Enqueue amend with metadata
    tx.execute(
        "INSERT INTO order_queue (order_id, action, actor, metadata) VALUES ($1, 'amend', 'api', $2)",
        &[&order_id, &metadata_json],
    )
    .await
    .map_err(|e| format!("enqueue amend: {}", e))?;

    // Audit log
    tx.execute(
        "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) VALUES ($1, $2, 'pending_amend', 'amend_request', 'api')",
        &[&order_id, &current_state_str],
    )
    .await
    .map_err(|e| format!("insert audit: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;

    debug!(order_id, "order amend enqueued atomically");

    Ok(())
}

/// Atomically decrease an order's quantity: lock row, verify state, update to PendingDecrease,
/// enqueue decrease with metadata — all in one transaction.
pub async fn atomic_decrease_order(
    pool: &Pool,
    order_id: i64,
    session_id: i64,
    reduce_by: Decimal,
) -> Result<(), String> {
    if reduce_by <= Decimal::ZERO {
        return Err("reduce_by must be positive".to_string());
    }

    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Lock the order row and get current state (scoped to session)
    let row = tx
        .query_opt(
            "SELECT state, quantity, filled_quantity FROM prediction_orders WHERE id = $1 AND session_id = $2 FOR UPDATE",
            &[&order_id, &session_id],
        )
        .await
        .map_err(|e| format!("get order: {}", e))?;

    let row = row.ok_or_else(|| "order not found".to_string())?;
    let current_state_str: String = row.get("state");
    let current_state = parse_state(&current_state_str);
    let quantity: Decimal = row.get("quantity");
    let filled_quantity: Decimal = row.get("filled_quantity");

    // Only decrease if in decreasable state
    if !matches!(
        current_state,
        OrderState::Acknowledged | OrderState::PartiallyFilled
    ) {
        return Err(format!(
            "cannot decrease order in {} state",
            current_state
        ));
    }

    // Validate reduce_by doesn't exceed remaining quantity
    let remaining = quantity - filled_quantity;
    if reduce_by >= remaining {
        return Err(format!(
            "reduce_by ({}) must be less than remaining quantity ({})",
            reduce_by, remaining
        ));
    }

    // Build metadata
    let metadata_json = serde_json::json!({"reduce_by": reduce_by.to_string()});

    // Update state to PendingDecrease
    tx.execute(
        "UPDATE prediction_orders SET state = 'pending_decrease' WHERE id = $1",
        &[&order_id],
    )
    .await
    .map_err(|e| format!("update state: {}", e))?;

    // Enqueue decrease with metadata
    tx.execute(
        "INSERT INTO order_queue (order_id, action, actor, metadata) VALUES ($1, 'decrease', 'api', $2)",
        &[&order_id, &metadata_json],
    )
    .await
    .map_err(|e| format!("enqueue decrease: {}", e))?;

    // Audit log
    tx.execute(
        "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) VALUES ($1, $2, 'pending_decrease', 'decrease_request', 'api')",
        &[&order_id, &current_state_str],
    )
    .await
    .map_err(|e| format!("insert audit: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;

    debug!(order_id, "order decrease enqueued atomically");

    Ok(())
}

/// Drain queue items during shutdown without transitioning orders through Submitted.
///
/// Directly deletes queue items for the given session and marks their orders as Rejected.
pub async fn drain_queue_for_shutdown(pool: &Pool, session_id: i64) -> Result<u64, String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Get queue items for this session's orders
    let rows = tx
        .query(
            "DELETE FROM order_queue WHERE order_id IN \
             (SELECT id FROM prediction_orders WHERE session_id = $1) \
             RETURNING order_id",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("drain queue: {}", e))?;

    let count = rows.len() as u64;

    // Mark non-terminal orders as rejected
    for row in &rows {
        let order_id: i64 = row.get("order_id");
        tx.execute(
            "UPDATE prediction_orders SET state = 'rejected', cancel_reason = 'shutdown' \
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

/// Drain ALL queue items during shutdown (across all sessions).
///
/// Used during pod shutdown to reject all pending orders regardless of session.
pub async fn drain_queue_for_shutdown_all(pool: &Pool) -> Result<u64, String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    let rows = tx
        .query(
            "DELETE FROM order_queue RETURNING order_id",
            &[],
        )
        .await
        .map_err(|e| format!("drain queue all: {}", e))?;

    let count = rows.len() as u64;

    for row in &rows {
        let order_id: i64 = row.get("order_id");
        tx.execute(
            "UPDATE prediction_orders SET state = 'rejected', cancel_reason = 'shutdown' \
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

/// Local position computed from filled orders in a session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalPosition {
    pub ticker: String,
    /// Net quantity: positive = long, negative = short
    pub net_quantity: Decimal,
    pub buy_filled: Decimal,
    pub sell_filled: Decimal,
}

/// Compute local positions from all filled orders in a session.
///
/// Groups by ticker, sums Buy filled_quantity (positive) and Sell filled_quantity (negative).
pub async fn compute_local_positions(
    pool: &Pool,
    session_id: i64,
) -> Result<Vec<LocalPosition>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT ticker, action, SUM(filled_quantity) as total_filled \
             FROM prediction_orders \
             WHERE session_id = $1 AND filled_quantity > 0 \
             GROUP BY ticker, action \
             ORDER BY ticker",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("compute local positions: {}", e))?;

    // Aggregate by ticker
    let mut map: std::collections::HashMap<String, (Decimal, Decimal)> =
        std::collections::HashMap::new();
    for row in &rows {
        let ticker: String = row.get("ticker");
        let action_str: String = row.get("action");
        let total: Decimal = row.get("total_filled");
        let entry = map.entry(ticker).or_insert((Decimal::ZERO, Decimal::ZERO));
        match action_str.as_str() {
            "buy" => entry.0 += total,
            "sell" => entry.1 += total,
            _ => {}
        }
    }

    let mut positions: Vec<LocalPosition> = map
        .into_iter()
        .map(|(ticker, (buy, sell))| LocalPosition {
            ticker,
            net_quantity: buy - sell,
            buy_filled: buy,
            sell_filled: sell,
        })
        .collect();
    positions.sort_by(|a, b| a.ticker.cmp(&b.ticker));
    Ok(positions)
}

/// Compute local positions aggregated across all active sessions for an exchange+environment.
pub async fn compute_all_local_positions(
    pool: &Pool,
    exchange: &str,
    environment: &str,
) -> Result<Vec<LocalPosition>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT o.ticker, o.action, SUM(o.filled_quantity) as total_filled \
             FROM prediction_orders o \
             JOIN sessions s ON o.session_id = s.id \
             WHERE s.exchange = $1 AND s.environment = $2 AND s.closed_at IS NULL AND o.filled_quantity > 0 \
             GROUP BY o.ticker, o.action \
             ORDER BY o.ticker",
            &[&exchange, &environment],
        )
        .await
        .map_err(|e| format!("compute all local positions: {}", e))?;

    let mut map: std::collections::HashMap<String, (Decimal, Decimal)> =
        std::collections::HashMap::new();
    for row in &rows {
        let ticker: String = row.get("ticker");
        let action_str: String = row.get("action");
        let total: Decimal = row.get("total_filled");
        let entry = map.entry(ticker).or_insert((Decimal::ZERO, Decimal::ZERO));
        match action_str.as_str() {
            "buy" => entry.0 += total,
            "sell" => entry.1 += total,
            _ => {}
        }
    }

    let mut positions: Vec<LocalPosition> = map
        .into_iter()
        .map(|(ticker, (buy, sell))| LocalPosition {
            ticker,
            net_quantity: buy - sell,
            buy_filled: buy,
            sell_filled: sell,
        })
        .collect();
    positions.sort_by(|a, b| a.ticker.cmp(&b.ticker));
    Ok(positions)
}

/// Parameters for creating a synthetic order from an external fill.
pub struct ExternalOrderParams<'a> {
    pub session_id: i64,
    pub exchange_order_id: &'a str,
    pub ticker: &'a str,
    pub side: Side,
    pub action: Action,
    pub quantity: Decimal,
    pub price_dollars: Decimal,
}

/// Create a synthetic order for an external fill (placed on exchange website, not via harman).
///
/// If an order with the same exchange_order_id already exists in this session, returns
/// its ID instead of creating a duplicate (handles partial fills on the same order).
///
/// The order is created in 'filled' state with actor='external'.
pub async fn create_external_order(
    pool: &Pool,
    params: &ExternalOrderParams<'_>,
) -> Result<i64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    // Check if we already have an order for this exchange_order_id in this session
    let existing = client
        .query_opt(
            "SELECT id FROM prediction_orders WHERE exchange_order_id = $1 AND session_id = $2",
            &[&params.exchange_order_id, &params.session_id],
        )
        .await
        .map_err(|e| format!("check existing external order: {}", e))?;

    if let Some(row) = existing {
        return Ok(row.get("id"));
    }

    // Create new synthetic order
    let client_order_id = Uuid::new_v4();
    let row = client
        .query_one(
            "INSERT INTO prediction_orders \
             (session_id, client_order_id, exchange_order_id, ticker, side, action, \
              quantity, price_dollars, filled_quantity, time_in_force, state) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'gtc', 'filled') \
             RETURNING id",
            &[
                &params.session_id,
                &client_order_id,
                &params.exchange_order_id,
                &params.ticker,
                &params.side.to_string(),
                &params.action.to_string(),
                &params.quantity,
                &params.price_dollars,
                &params.quantity, // filled_quantity = quantity (fully filled)
            ],
        )
        .await
        .map_err(|e| format!("insert external order: {}", e))?;

    let order_id: i64 = row.get("id");

    // Audit log
    let _ = client
        .execute(
            "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
             VALUES ($1, 'none', 'filled', 'external_import', 'external')",
            &[&order_id],
        )
        .await;

    info!(
        order_id,
        exchange_order_id = params.exchange_order_id,
        ticker = params.ticker,
        "created synthetic order for external fill"
    );

    Ok(order_id)
}

/// Create a synthetic order for an external resting order (placed on exchange website).
///
/// Unlike `create_external_order()` (for fills), this creates the order in 'acknowledged'
/// state with filled_quantity=0, representing a live resting order on the exchange.
///
/// If an order with the same exchange_order_id already exists, returns its ID.
pub async fn create_external_resting_order(
    pool: &Pool,
    params: &ExternalOrderParams<'_>,
) -> Result<i64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    // Check if we already have this order
    let existing = client
        .query_opt(
            "SELECT id FROM prediction_orders WHERE exchange_order_id = $1 AND session_id = $2",
            &[&params.exchange_order_id, &params.session_id],
        )
        .await
        .map_err(|e| format!("check existing external resting order: {}", e))?;

    if let Some(row) = existing {
        return Ok(row.get("id"));
    }

    let client_order_id = Uuid::new_v4();
    let row = client
        .query_one(
            "INSERT INTO prediction_orders \
             (session_id, client_order_id, exchange_order_id, ticker, side, action, \
              quantity, price_dollars, filled_quantity, time_in_force, state) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, 'gtc', 'acknowledged') \
             RETURNING id",
            &[
                &params.session_id,
                &client_order_id,
                &params.exchange_order_id,
                &params.ticker,
                &params.side.to_string(),
                &params.action.to_string(),
                &params.quantity,
                &params.price_dollars,
            ],
        )
        .await
        .map_err(|e| format!("insert external resting order: {}", e))?;

    let order_id: i64 = row.get("id");

    // Audit log
    let _ = client
        .execute(
            "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
             VALUES ($1, 'none', 'acknowledged', 'external_import', 'external')",
            &[&order_id],
        )
        .await;

    info!(
        order_id,
        exchange_order_id = params.exchange_order_id,
        ticker = params.ticker,
        "created synthetic order for external resting order"
    );

    Ok(order_id)
}

// --- Helper parsers ---

/// Create an order group with its legs atomically.
///
/// Each leg is an `(OrderRequest, LegRole, OrderState)` tuple. Pending legs are
/// queued for submission; staged legs wait for trigger activation. Risk check
/// applies only to pending legs (staged legs are excluded by design).
pub async fn create_order_group(
    pool: &Pool,
    session_id: i64,
    group_type: GroupType,
    legs: &[(OrderRequest, LegRole, OrderState)],
    risk_limits: &RiskLimits,
) -> Result<(OrderGroup, Vec<Order>), EnqueueError> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| EnqueueError::Database(format!("pool error: {}", e)))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| EnqueueError::Database(format!("begin tx: {}", e)))?;

    // Lock open order rows to serialize concurrent enqueues, then compute risk.
    tx.query(
        "SELECT id FROM prediction_orders \
         WHERE session_id = $1 AND state IN ('pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease') \
         FOR UPDATE",
        &[&session_id],
    )
    .await
    .map_err(|e| EnqueueError::Database(format!("risk lock: {}", e)))?;

    let risk_row = tx
        .query_one(
            "SELECT COALESCE(SUM(price_dollars * (quantity - filled_quantity)), 0) as open_notional \
             FROM prediction_orders \
             WHERE session_id = $1 AND state IN ('pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease')",
            &[&session_id],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("risk query: {}", e)))?;

    let open_notional: Decimal = risk_row.get::<_, Decimal>("open_notional");

    // Query per-session risk limit; fall back to global
    let session_row = tx
        .query_one(
            "SELECT max_notional FROM sessions WHERE id = $1",
            &[&session_id],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("session risk query: {}", e)))?;

    let effective_max = match session_row.get::<_, Option<Decimal>>("max_notional") {
        Some(session_max) => session_max,
        None => risk_limits.max_notional,
    };

    // Compute notional for pending legs only (staged legs excluded from risk)
    let mut pending_notional = Decimal::ZERO;
    for (req, _role, state) in legs {
        if state.is_open() {
            pending_notional += req.notional();
        }
    }

    if open_notional + pending_notional > effective_max {
        return Err(EnqueueError::RiskCheck(
            crate::error::RiskCheckError::MaxNotionalExceeded {
                current: open_notional,
                requested: pending_notional,
                limit: effective_max,
            },
        ));
    }

    // Create the group
    let group_row = tx
        .query_one(
            "INSERT INTO order_groups (session_id, group_type) VALUES ($1, $2) \
             RETURNING id, session_id, group_type, state, created_at, updated_at",
            &[&session_id, &group_type.to_string()],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("insert group: {}", e)))?;

    let group_id: i64 = group_row.get("id");
    let group = OrderGroup {
        id: group_id,
        session_id: group_row.get("session_id"),
        group_type: parse_group_type(group_row.get("group_type")),
        state: parse_group_state(group_row.get("state")),
        created_at: group_row.get("created_at"),
        updated_at: group_row.get("updated_at"),
    };

    let mut orders = Vec::with_capacity(legs.len());
    for (req, role, initial_state) in legs {
        let state_str = initial_state.to_string();
        let role_str = role.to_string();

        let row = tx
            .query_one(
                "INSERT INTO prediction_orders \
                 (session_id, client_order_id, ticker, side, action, quantity, price_dollars, \
                  time_in_force, state, group_id, leg_role) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
                 RETURNING id, created_at, updated_at",
                &[
                    &session_id,
                    &req.client_order_id,
                    &req.ticker,
                    &req.side.to_string(),
                    &req.action.to_string(),
                    &req.quantity,
                    &req.price_dollars,
                    &req.time_in_force.to_string(),
                    &state_str,
                    &group_id,
                    &role_str,
                ],
            )
            .await
            .map_err(|e| {
                if let Some(db_err) = e.as_db_error() {
                    if db_err.code() == &tokio_postgres::error::SqlState::UNIQUE_VIOLATION {
                        return EnqueueError::DuplicateClientOrderId(req.client_order_id);
                    }
                }
                EnqueueError::Database(format!("insert leg order: {}", e))
            })?;

        let order_id: i64 = row.get("id");
        let created_at: DateTime<Utc> = row.get("created_at");
        let updated_at: DateTime<Utc> = row.get("updated_at");

        // Queue pending legs for submission
        if *initial_state == OrderState::Pending {
            tx.execute(
                "INSERT INTO order_queue (order_id, action, actor) VALUES ($1, 'submit', 'api')",
                &[&order_id],
            )
            .await
            .map_err(|e| EnqueueError::Database(format!("insert queue: {}", e)))?;
        }

        // Audit log
        tx.execute(
            "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
             VALUES ($1, 'none', $2, 'created', 'api')",
            &[&order_id, &state_str],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("insert audit: {}", e)))?;

        orders.push(Order {
            id: order_id,
            session_id,
            client_order_id: req.client_order_id,
            exchange_order_id: None,
            ticker: req.ticker.clone(),
            side: req.side,
            action: req.action,
            quantity: req.quantity,
            price_dollars: req.price_dollars,
            filled_quantity: Decimal::ZERO,
            time_in_force: req.time_in_force,
            state: *initial_state,
            cancel_reason: None,
            group_id: Some(group_id),
            leg_role: Some(*role),
            created_at,
            updated_at,
        });
    }

    tx.commit()
        .await
        .map_err(|e| EnqueueError::Database(format!("commit: {}", e)))?;

    debug!(group_id, group_type = %group_type, "order group created");

    Ok((group, orders))
}

/// Activate a staged order: Staged → Pending + insert queue item.
///
/// No risk check — exit legs close the entry position.
pub async fn activate_staged_order(
    pool: &Pool,
    order_id: i64,
    session_id: i64,
) -> Result<(), String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    let updated = tx
        .execute(
            "UPDATE prediction_orders SET state = 'pending' \
             WHERE id = $1 AND session_id = $2 AND state = 'staged'",
            &[&order_id, &session_id],
        )
        .await
        .map_err(|e| format!("activate staged: {}", e))?;

    if updated == 0 {
        return Err(format!("order {} not in staged state", order_id));
    }

    tx.execute(
        "INSERT INTO order_queue (order_id, action, actor) VALUES ($1, 'submit', 'trigger')",
        &[&order_id],
    )
    .await
    .map_err(|e| format!("insert queue: {}", e))?;

    tx.execute(
        "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
         VALUES ($1, 'staged', 'pending', 'activate', 'trigger')",
        &[&order_id],
    )
    .await
    .map_err(|e| format!("insert audit: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;

    info!(order_id, "staged order activated");
    Ok(())
}

/// Get all orders for a group.
pub async fn get_group_orders(
    pool: &Pool,
    group_id: i64,
    session_id: i64,
) -> Result<Vec<Order>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT id, session_id, client_order_id, exchange_order_id, \
                    ticker, side, action, quantity, price_dollars, \
                    filled_quantity, time_in_force, state, cancel_reason, \
                    group_id, leg_role, created_at, updated_at \
             FROM prediction_orders \
             WHERE group_id = $1 AND session_id = $2 \
             ORDER BY id",
            &[&group_id, &session_id],
        )
        .await
        .map_err(|e| format!("get group orders: {}", e))?;

    Ok(rows.iter().map(row_to_order).collect())
}

/// Get active groups with at least one terminal leg for trigger evaluation.
pub async fn get_groups_needing_evaluation(
    pool: &Pool,
    session_id: i64,
) -> Result<Vec<(OrderGroup, Vec<Order>)>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    // Find active groups that have at least one terminal order
    let group_rows = client
        .query(
            "SELECT DISTINCT g.id, g.session_id, g.group_type, g.state, g.created_at, g.updated_at \
             FROM order_groups g \
             JOIN prediction_orders o ON o.group_id = g.id \
             WHERE g.state = 'active' AND g.session_id = $1 \
               AND o.state IN ('filled', 'cancelled', 'rejected', 'expired') \
             ORDER BY g.id",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("get groups needing eval: {}", e))?;

    debug!(
        session_id,
        groups_found = group_rows.len(),
        "get_groups_needing_evaluation"
    );

    let mut results = Vec::new();
    for grow in &group_rows {
        let group = row_to_group(grow);
        let order_rows = client
            .query(
                "SELECT id, session_id, client_order_id, exchange_order_id, \
                        ticker, side, action, quantity, price_dollars, \
                        filled_quantity, time_in_force, state, cancel_reason, \
                        group_id, leg_role, created_at, updated_at \
                 FROM prediction_orders \
                 WHERE group_id = $1 AND session_id = $2 \
                 ORDER BY id",
                &[&group.id, &session_id],
            )
            .await
            .map_err(|e| format!("get group orders for eval: {}", e))?;

        let orders: Vec<Order> = order_rows.iter().map(row_to_order).collect();
        results.push((group, orders));
    }

    Ok(results)
}

/// Update an order group's state.
pub async fn update_group_state(
    pool: &Pool,
    group_id: i64,
    state: GroupState,
) -> Result<(), String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    client
        .execute(
            "UPDATE order_groups SET state = $1 WHERE id = $2",
            &[&state.to_string(), &group_id],
        )
        .await
        .map_err(|e| format!("update group state: {}", e))?;

    Ok(())
}

/// List groups for a session, with optional state filter.
pub async fn list_groups(
    pool: &Pool,
    session_id: i64,
    state_filter: Option<GroupState>,
) -> Result<Vec<OrderGroup>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = if let Some(state) = state_filter {
        client
            .query(
                "SELECT id, session_id, group_type, state, created_at, updated_at \
                 FROM order_groups WHERE session_id = $1 AND state = $2 ORDER BY id",
                &[&session_id, &state.to_string()],
            )
            .await
    } else {
        client
            .query(
                "SELECT id, session_id, group_type, state, created_at, updated_at \
                 FROM order_groups WHERE session_id = $1 ORDER BY id",
                &[&session_id],
            )
            .await
    }
    .map_err(|e| format!("list groups: {}", e))?;

    Ok(rows.iter().map(row_to_group).collect())
}

/// Get a group by ID, scoped to a session.
pub async fn get_group(
    pool: &Pool,
    group_id: i64,
    session_id: i64,
) -> Result<Option<OrderGroup>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_opt(
            "SELECT id, session_id, group_type, state, created_at, updated_at \
             FROM order_groups WHERE id = $1 AND session_id = $2",
            &[&group_id, &session_id],
        )
        .await
        .map_err(|e| format!("get group: {}", e))?;

    Ok(row.as_ref().map(row_to_group))
}

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
        price_dollars: row.get("price_dollars"),
        filled_quantity: row.get("filled_quantity"),
        time_in_force: parse_tif(row.get("time_in_force")),
        state: parse_state(row.get("state")),
        cancel_reason: row
            .get::<_, Option<String>>("cancel_reason")
            .map(|s| parse_cancel_reason(&s)),
        group_id: row.get("group_id"),
        leg_role: row
            .get::<_, Option<String>>("leg_role")
            .map(|s| parse_leg_role(&s)),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_group(row: &tokio_postgres::Row) -> OrderGroup {
    OrderGroup {
        id: row.get("id"),
        session_id: row.get("session_id"),
        group_type: parse_group_type(row.get("group_type")),
        state: parse_group_state(row.get("state")),
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
        "pending_amend" => OrderState::PendingAmend,
        "pending_decrease" => OrderState::PendingDecrease,
        "cancelled" => OrderState::Cancelled,
        "rejected" => OrderState::Rejected,
        "expired" => OrderState::Expired,
        "staged" => OrderState::Staged,
        _ => {
            warn!(state = s, "unknown order state in DB, defaulting to Pending");
            OrderState::Pending
        }
    }
}

fn parse_leg_role(s: &str) -> LegRole {
    match s {
        "entry" => LegRole::Entry,
        "take_profit" => LegRole::TakeProfit,
        "stop_loss" => LegRole::StopLoss,
        "oco_leg" => LegRole::OcoLeg,
        _ => {
            warn!(value = s, "unknown leg_role in DB, defaulting to Entry");
            LegRole::Entry
        }
    }
}

fn parse_group_type(s: &str) -> GroupType {
    match s {
        "bracket" => GroupType::Bracket,
        "oco" => GroupType::Oco,
        _ => {
            warn!(value = s, "unknown group_type in DB, defaulting to Bracket");
            GroupType::Bracket
        }
    }
}

fn parse_group_state(s: &str) -> GroupState {
    match s {
        "active" => GroupState::Active,
        "completed" => GroupState::Completed,
        "cancelled" => GroupState::Cancelled,
        _ => {
            warn!(value = s, "unknown group_state in DB, defaulting to Active");
            GroupState::Active
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
