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
use crate::state::{apply_event, OrderEvent, OrderState};
use crate::types::{
    Action, CancelReason, GroupState, GroupType, LegRole, MarketResult, Order, OrderGroup,
    OrderRequest, QueueAction, Settlement, Side, TimeInForce,
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

    // Check if 008 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '008_environment_test'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 008: {}", e))?;

    if row.is_none() {
        let migration_008 = include_str!("../migrations/008_environment_test.sql");
        client
            .batch_execute(migration_008)
            .await
            .map_err(|e| format!("migration 008 failed: {}", e))?;
        info!("migration 008_environment_test applied");
    }

    // Check if 009 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '009_stable_sessions'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 009: {}", e))?;

    if row.is_none() {
        let migration_009 = include_str!("../migrations/009_stable_sessions.sql");
        client
            .batch_execute(migration_009)
            .await
            .map_err(|e| format!("migration 009 failed: {}", e))?;
        info!("migration 009_stable_sessions applied");
    }

    // Check if 010 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '010_settlements'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 010: {}", e))?;

    if row.is_none() {
        let migration_010 = include_str!("../migrations/010_settlements.sql");
        client
            .batch_execute(migration_010)
            .await
            .map_err(|e| format!("migration 010 failed: {}", e))?;
        info!("migration 010_settlements applied");
    }

    // Check if 011 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '011_exchange_audit_log'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 011: {}", e))?;

    if row.is_none() {
        let migration_011 = include_str!("../migrations/011_exchange_audit_log.sql");
        client
            .batch_execute(migration_011)
            .await
            .map_err(|e| format!("migration 011 failed: {}", e))?;
        info!("migration 011_exchange_audit_log applied");
    }

    // Check if 012 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '012_audit_event_id'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 012: {}", e))?;

    if row.is_none() {
        let migration_012 = include_str!("../migrations/012_audit_event_id.sql");
        client
            .batch_execute(migration_012)
            .await
            .map_err(|e| format!("migration 012 failed: {}", e))?;
        info!("migration 012_audit_event_id applied");
    }

    // Check if 013 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '013_daily_loss_limit'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 013: {}", e))?;

    if row.is_none() {
        let migration_013 = include_str!("../migrations/013_daily_loss_limit.sql");
        client
            .batch_execute(migration_013)
            .await
            .map_err(|e| format!("migration 013 failed: {}", e))?;
        info!("migration 013_daily_loss_limit applied");
    }

    // Check if 014 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '014_nullable_audit_session_id'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 014: {}", e))?;

    if row.is_none() {
        let migration_014 = include_str!("../migrations/014_nullable_audit_session_id.sql");
        client
            .batch_execute(migration_014)
            .await
            .map_err(|e| format!("migration 014 failed: {}", e))?;
        info!("migration 014_nullable_audit_session_id applied");
    }

    // Check if 015 is applied
    let row = client
        .query_opt(
            "SELECT version FROM schema_migrations WHERE version = '015_remove_filled_quantity'",
            &[],
        )
        .await
        .map_err(|e| format!("check migration 015: {}", e))?;

    if row.is_none() {
        let migration_015 = include_str!("../migrations/015_remove_filled_quantity.sql");
        client
            .batch_execute(migration_015)
            .await
            .map_err(|e| format!("migration 015 failed: {}", e))?;
        info!("migration 015_remove_filled_quantity applied");
    }

    info!("database migrations applied successfully");
    Ok(())
}

/// Batch INSERT audit events into exchange_audit_log.
/// JSONB columns are serialized to strings and cast in SQL.
/// Each event has a UUID event_id; ON CONFLICT DO NOTHING makes retries idempotent.
/// Per-row inserts: one bad event (e.g. FK violation) won't poison the whole batch.
pub async fn batch_insert_audit(
    pool: &Pool,
    events: &[crate::audit::AuditEvent],
) -> Result<u64, String> {
    let client = pool.get().await.map_err(|e| format!("pool: {e}"))?;
    let stmt = client
        .prepare(
            "INSERT INTO exchange_audit_log
             (event_id, session_id, order_id, category, action, endpoint, status_code, duration_ms,
              request, response, outcome, error_msg, metadata)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                     $9::TEXT::JSONB, $10::TEXT::JSONB, $11, $12, $13::TEXT::JSONB)
             ON CONFLICT (event_id) WHERE event_id IS NOT NULL DO NOTHING",
        )
        .await
        .map_err(|e| format!("prepare: {e:?}"))?;

    let mut count = 0u64;
    let mut skipped = 0u64;
    for event in events {
        let request_json = event
            .request
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        let response_json = event
            .response
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        let metadata_json = event
            .metadata
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        match client.execute(
                &stmt,
                &[
                    &event.event_id,
                    &event.session_id,
                    &event.order_id,
                    &event.category,
                    &event.action,
                    &event.endpoint,
                    &event.status_code,
                    &event.duration_ms,
                    &request_json,
                    &response_json,
                    &event.outcome,
                    &event.error_msg,
                    &metadata_json,
                ],
            )
            .await
        {
            Ok(_) => count += 1,
            Err(e) => {
                warn!(
                    category = event.category,
                    action = %event.action,
                    session_id = ?event.session_id,
                    "skipping audit event: {e:?}"
                );
                skipped += 1;
            }
        }
    }
    if skipped > 0 {
        warn!(skipped, inserted = count, "some audit events skipped");
    }
    Ok(count)
}

/// Map a (from_state, to_state) pair to the OrderEvent that would cause it.
/// Used to validate transitions through the state machine.
fn infer_event(from: OrderState, to: OrderState) -> Result<OrderEvent, String> {
    match (from, to) {
        (_, OrderState::Submitted) => Ok(OrderEvent::Submit),
        (_, OrderState::Acknowledged) => Ok(OrderEvent::Acknowledge {
            exchange_order_id: String::new(),
        }),
        (_, OrderState::Rejected) => Ok(OrderEvent::Reject {
            reason: String::new(),
        }),
        (_, OrderState::Filled) => Ok(OrderEvent::Fill {
            filled_qty: Decimal::ZERO,
        }),
        (_, OrderState::PartiallyFilled) => Ok(OrderEvent::PartialFill {
            filled_qty: Decimal::ZERO,
        }),
        (_, OrderState::PendingCancel) => Ok(OrderEvent::CancelRequest),
        (_, OrderState::PendingAmend) => Ok(OrderEvent::AmendRequest),
        (_, OrderState::PendingDecrease) => Ok(OrderEvent::DecreaseRequest),
        (OrderState::PendingCancel, OrderState::Cancelled) => Ok(OrderEvent::CancelConfirm),
        (OrderState::Staged, OrderState::Cancelled) => Ok(OrderEvent::CancelRequest),
        (OrderState::Pending, OrderState::Cancelled) => Ok(OrderEvent::CancelRequest),
        (_, OrderState::Expired) => Ok(OrderEvent::Expire),
        (OrderState::Staged, OrderState::Pending) => Ok(OrderEvent::Activate),
        _ => Err(format!("unmapped transition: {} -> {}", from, to)),
    }
}

/// Validate a state transition through the state machine.
/// Returns the validated new state or an error.
fn validate_transition(from: OrderState, to: OrderState) -> Result<OrderState, String> {
    let event = infer_event(from, to)?;
    apply_event(from, &event).map_err(|e| format!("{}", e))
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
         WHERE session_id = $1 AND state IN ('staged', 'pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease') \
         FOR UPDATE",
        &[&session_id],
    )
    .await
    .map_err(|e| EnqueueError::Database(format!("risk lock: {}", e)))?;

    let risk_row = tx
        .query_one(
            "SELECT COALESCE(SUM(price_dollars * (quantity - filled_qty(id))), 0) as open_notional \
             FROM prediction_orders \
             WHERE session_id = $1 AND state IN ('staged', 'pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease')",
            &[&session_id],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("risk query: {}", e)))?;

    let open_notional: Decimal = risk_row.get::<_, Decimal>("open_notional");

    let risk_state = RiskState { open_notional };

    // Query per-session risk limits; fall back to global
    let session_row = tx
        .query_one(
            "SELECT max_notional, daily_loss_limit FROM sessions WHERE id = $1",
            &[&session_id],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("session risk query: {}", e)))?;

    let effective_limits = RiskLimits {
        max_notional: session_row
            .get::<_, Option<Decimal>>("max_notional")
            .unwrap_or(limits.max_notional),
        max_order_notional: limits.max_order_notional,
        daily_loss_limit: limits.daily_loss_limit,
    };

    // Risk check (fat-finger + aggregate notional)
    risk_state
        .check_order(request, &effective_limits)
        .map_err(EnqueueError::RiskCheck)?;

    // Daily loss check — query realized P&L from today's settlements
    let daily_loss_limit = match session_row.get::<_, Option<Decimal>>("daily_loss_limit") {
        Some(session_limit) => session_limit,
        None => limits.daily_loss_limit,
    };

    let pnl_row = tx
        .query_one(
            "SELECT COALESCE(SUM(revenue_dollars - COALESCE(value_dollars, 0)), 0) AS daily_pnl \
             FROM settlements \
             WHERE session_id = $1 AND settled_time >= CURRENT_DATE",
            &[&session_id],
        )
        .await
        .map_err(|e| EnqueueError::Database(format!("daily pnl query: {}", e)))?;

    let daily_pnl: Decimal = pnl_row.get::<_, Decimal>("daily_pnl");
    if daily_pnl < -daily_loss_limit {
        return Err(EnqueueError::RiskCheck(
            crate::error::RiskCheckError::DailyLossExceeded {
                daily_pnl,
                limit: daily_loss_limit,
            },
        ));
    }

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
    pub action: QueueAction,
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
                    filled_qty(o.id) as filled_quantity, o.time_in_force, o.state, o.cancel_reason, \
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
    let action_str: String = row.get("action");
    let action: QueueAction = action_str.parse().map_err(|e: String| {
        format!("queue_id={} order_id={}: {}", queue_id, order_id, e)
    })?;

    // Mark as processing
    tx.execute(
        "UPDATE order_queue SET processing = TRUE WHERE id = $1",
        &[&queue_id],
    )
    .await
    .map_err(|e| format!("mark processing: {}", e))?;

    // Update order state to submitted (for submit actions)
    if action == QueueAction::Submit {
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
        state: if action == QueueAction::Submit {
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
    let cancel_str = cancel_reason.map(|r| r.to_string());
    let cancel_ref = cancel_str.as_deref();

    tx.execute(
        "UPDATE prediction_orders SET state = $1, exchange_order_id = COALESCE($2, exchange_order_id), \
         cancel_reason = COALESCE($3, cancel_reason) \
         WHERE id = $4 AND session_id = $5",
        &[
            &state_str,
            &exchange_order_id,
            &cancel_ref,
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

/// Update an order after a successful amend on the exchange.
///
/// Wraps the state update (PendingAmend → Acknowledged), exchange_order_id swap,
/// price/quantity update, and audit log in a single transaction.
pub async fn update_amended_order(
    pool: &Pool,
    order_id: i64,
    session_id: i64,
    new_exchange_order_id: &str,
    new_price_dollars: Decimal,
    new_quantity: Decimal,
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
            "SELECT state FROM prediction_orders WHERE id = $1 AND session_id = $2 FOR UPDATE",
            &[&order_id, &session_id],
        )
        .await
        .map_err(|e| format!("get state: {}", e))?;
    let from_state: String = row.get("state");

    tx.execute(
        "UPDATE prediction_orders SET state = 'acknowledged', \
         exchange_order_id = $1, price_dollars = $2, quantity = $3 \
         WHERE id = $4 AND session_id = $5",
        &[
            &new_exchange_order_id,
            &new_price_dollars,
            &new_quantity,
            &order_id,
            &session_id,
        ],
    )
    .await
    .map_err(|e| format!("update amended order: {}", e))?;

    tx.execute(
        "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
         VALUES ($1, $2, 'acknowledged', 'amend_confirm', 'pump')",
        &[&order_id, &from_state],
    )
    .await
    .map_err(|e| format!("insert amend audit: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;

    Ok(())
}

/// Update an order after a successful decrease on the exchange.
///
/// Wraps the state update (PendingDecrease → Acknowledged), quantity reduction,
/// and audit log in a single transaction.
pub async fn update_decreased_order(
    pool: &Pool,
    order_id: i64,
    session_id: i64,
    reduce_by: Decimal,
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
            "SELECT state FROM prediction_orders WHERE id = $1 AND session_id = $2 FOR UPDATE",
            &[&order_id, &session_id],
        )
        .await
        .map_err(|e| format!("get state: {}", e))?;
    let from_state: String = row.get("state");

    tx.execute(
        "UPDATE prediction_orders SET state = 'acknowledged', \
         quantity = quantity - $1 \
         WHERE id = $2 AND session_id = $3",
        &[&reduce_by, &order_id, &session_id],
    )
    .await
    .map_err(|e| format!("update decreased order: {}", e))?;

    tx.execute(
        "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
         VALUES ($1, $2, 'acknowledged', 'decrease_confirm', 'pump')",
        &[&order_id, &from_state],
    )
    .await
    .map_err(|e| format!("insert decrease audit: {}", e))?;

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
/// Find the best existing session for (exchange, environment) at startup.
/// Prefers authenticated sessions (non-NULL key_prefix) over placeholders.
/// Returns None if no session exists (first boot).
pub async fn find_startup_session(
    pool: &Pool,
    exchange: &str,
    environment: &str,
) -> Result<Option<i64>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT id, api_key_prefix FROM sessions \
             WHERE exchange = $1 AND environment = $2 \
             ORDER BY (api_key_prefix IS NOT NULL) DESC, created_at DESC \
             LIMIT 1",
            &[&exchange, &environment],
        )
        .await
        .map_err(|e| format!("find startup session: {:?}", e))?;

    match rows.first() {
        Some(row) => {
            let id: i64 = row.get("id");
            let prefix: Option<String> = row.get("api_key_prefix");
            info!(session_id = id, key_prefix = ?prefix, exchange, environment, "found existing session for startup");
            Ok(Some(id))
        }
        None => Ok(None),
    }
}

/// Clean up orphaned NULL-key sessions by moving their orders to the target session
/// and deleting the orphan. Called at startup when an authenticated session exists.
pub async fn absorb_null_key_sessions(
    pool: &Pool,
    exchange: &str,
    environment: &str,
    target_session_id: i64,
) -> Result<u64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let orphans: Vec<i64> = client
        .query(
            "SELECT id FROM sessions \
             WHERE exchange = $1 AND environment = $2 AND api_key_prefix IS NULL AND id != $3",
            &[&exchange, &environment, &target_session_id],
        )
        .await
        .map_err(|e| format!("find orphans: {:?}", e))?
        .iter()
        .map(|r| r.get("id"))
        .collect();

    if orphans.is_empty() {
        return Ok(0);
    }

    let mut total_moved = 0u64;
    for orphan_id in &orphans {
        let moved = client
            .execute(
                "UPDATE prediction_orders SET session_id = $1 WHERE session_id = $2",
                &[&target_session_id, orphan_id],
            )
            .await
            .map_err(|e| format!("move orders: {:?}", e))?;
        total_moved += moved;

        client
            .execute("DELETE FROM sessions WHERE id = $1", &[orphan_id])
            .await
            .map_err(|e| format!("delete orphan: {:?}", e))?;

        info!(orphan_session_id = orphan_id, target_session_id, moved_orders = moved, "absorbed orphan NULL-key session");
    }

    Ok(total_moved)
}

/// Stable sessions: uses INSERT ON CONFLICT DO NOTHING + SELECT on the natural key
/// (exchange, environment, COALESCE(api_key_prefix, '__none__')). Same inputs always
/// return the same session ID. No advisory locks needed.
pub async fn get_or_create_session(
    pool: &Pool,
    exchange: &str,
    environment: &str,
    key_prefix: Option<&str>,
) -> Result<i64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    // Upsert: create if not exists, no-op on conflict
    client
        .execute(
            "INSERT INTO sessions (exchange, environment, api_key_prefix) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (exchange, environment, COALESCE(api_key_prefix, '__none__')) \
             DO NOTHING",
            &[&exchange, &environment, &key_prefix],
        )
        .await
        .map_err(|e| format!("upsert session: {:?}", e))?;

    // Select the stable session
    let row = match key_prefix {
        Some(prefix) => {
            client
                .query_one(
                    "SELECT id FROM sessions \
                     WHERE exchange = $1 AND environment = $2 AND api_key_prefix = $3",
                    &[&exchange, &environment, &prefix],
                )
                .await
        }
        None => {
            client
                .query_one(
                    "SELECT id FROM sessions \
                     WHERE exchange = $1 AND environment = $2 AND api_key_prefix IS NULL",
                    &[&exchange, &environment],
                )
                .await
        }
    }
    .map_err(|e| format!("select session: {:?}", e))?;

    let id: i64 = row.get("id");
    info!(session_id = id, exchange, environment, key_prefix, "stable session ready");
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
                    filled_qty(id) as filled_quantity, time_in_force, state, cancel_reason, \
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
                    filled_qty(id) as filled_quantity, time_in_force, state, cancel_reason, \
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
                    filled_qty(id) as filled_quantity, time_in_force, state, cancel_reason, \
                    group_id, leg_role, created_at, updated_at \
             FROM prediction_orders WHERE client_order_id = $1 AND session_id = $2",
            &[&client_order_id, &session_id],
        )
        .await
        .map_err(|e| format!("get order by cid: {}", e))?;

    Ok(row.as_ref().map(row_to_order))
}

/// Find an order by its exchange-assigned order ID.
///
/// No session_id filter — each harman instance has its own DB,
/// and exchange_order_id is unique within it.
pub async fn find_order_by_exchange_id(
    pool: &Pool,
    exchange_order_id: &str,
) -> Result<Option<Order>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_opt(
            "SELECT id, session_id, client_order_id, exchange_order_id, \
                    ticker, side, action, quantity, price_dollars, \
                    filled_qty(id) as filled_quantity, time_in_force, state, cancel_reason, \
                    group_id, leg_role, created_at, updated_at \
             FROM prediction_orders WHERE exchange_order_id = $1",
            &[&exchange_order_id],
        )
        .await
        .map_err(|e| format!("find order by exchange_id: {}", e))?;

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
                        filled_qty(id) as filled_quantity, time_in_force, state, cancel_reason, \
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
                        filled_qty(id) as filled_quantity, time_in_force, state, cancel_reason, \
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
            "SELECT COALESCE(SUM(price_dollars * (quantity - filled_qty(id))), 0) as open_notional \
             FROM prediction_orders \
             WHERE session_id = $1 AND state IN ('staged', 'pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease')",
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
}

/// List all sessions for an exchange+environment, with open_notional for each.
/// Sessions are permanent (stable sessions) — all sessions are always active.
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
                    created_at::text \
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

        let open_notional = match compute_risk_state(pool, id).await {
            Ok(rs) => rs.open_notional,
            Err(_) => Decimal::ZERO,
        };

        sessions.push(SessionInfo {
            id,
            api_key_prefix: row.get("api_key_prefix"),
            display_name: row.get("display_name"),
            max_notional: max_notional.map(|d| d.to_string()),
            suspended: is_suspended(id),
            open_notional: open_notional.to_string(),
            created_at: row.get("created_at"),
        });
    }

    Ok(sessions)
}

/// Update the per-session risk limit (NULL = reset to global).
/// Scoped to exchange+environment so an admin cannot modify sessions belonging to another instance.
pub async fn update_session_risk(
    pool: &Pool,
    session_id: i64,
    exchange: &str,
    environment: &str,
    max_notional: Option<Decimal>,
) -> Result<bool, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let count = client
        .execute(
            "UPDATE sessions SET max_notional = $2 WHERE id = $1 AND exchange = $3 AND environment = $4",
            &[&session_id, &max_notional, &exchange, &environment],
        )
        .await
        .map_err(|e| format!("update session risk: {}", e))?;

    Ok(count > 0)
}

/// List all session IDs for an exchange+environment.
/// Sessions are permanent (stable sessions) — no closed_at filter needed.
pub async fn list_session_ids(
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
            "SELECT id FROM sessions WHERE exchange = $1 AND environment = $2 ORDER BY id",
            &[&exchange, &environment],
        )
        .await
        .map_err(|e| format!("list sessions: {}", e))?;

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

    let cancel_str = cancel_reason.to_string();

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
            "SELECT state, quantity, filled_qty(id) as filled_quantity FROM prediction_orders WHERE id = $1 AND session_id = $2 FOR UPDATE",
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
/// Groups by ticker, sums Buy fills (positive) and Sell fills (negative).
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
            "SELECT o.ticker, o.action, SUM(f.quantity) as total_filled \
             FROM prediction_orders o \
             JOIN fills f ON f.order_id = o.id \
             WHERE o.session_id = $1 \
               AND o.ticker NOT IN (SELECT ticker FROM settlements WHERE session_id = $1) \
             GROUP BY o.ticker, o.action \
             ORDER BY o.ticker",
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
            "SELECT o.ticker, o.action, SUM(f.quantity) as total_filled \
             FROM prediction_orders o \
             JOIN sessions s ON o.session_id = s.id \
             JOIN fills f ON f.order_id = o.id \
             WHERE s.exchange = $1 AND s.environment = $2 \
               AND o.ticker NOT IN (SELECT ticker FROM settlements WHERE session_id = o.session_id) \
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
              quantity, price_dollars, time_in_force, state) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'gtc', 'filled') \
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
/// state representing a live resting order on the exchange.
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
              quantity, price_dollars, time_in_force, state) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'gtc', 'acknowledged') \
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
         WHERE session_id = $1 AND state IN ('staged', 'pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease') \
         FOR UPDATE",
        &[&session_id],
    )
    .await
    .map_err(|e| EnqueueError::Database(format!("risk lock: {}", e)))?;

    let risk_row = tx
        .query_one(
            "SELECT COALESCE(SUM(price_dollars * (quantity - filled_qty(id))), 0) as open_notional \
             FROM prediction_orders \
             WHERE session_id = $1 AND state IN ('staged', 'pending', 'submitted', 'acknowledged', 'partially_filled', 'pending_cancel', 'pending_amend', 'pending_decrease')",
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

    // Fat-finger check per leg + compute total notional for non-terminal legs
    let mut legs_notional = Decimal::ZERO;
    for (req, _role, state) in legs {
        let leg_notional = req.notional();
        if leg_notional > risk_limits.max_order_notional {
            return Err(EnqueueError::RiskCheck(
                crate::error::RiskCheckError::MaxOrderNotionalExceeded {
                    order_notional: leg_notional,
                    limit: risk_limits.max_order_notional,
                },
            ));
        }
        if state.is_open() {
            legs_notional += leg_notional;
        }
    }

    if open_notional + legs_notional > effective_max {
        return Err(EnqueueError::RiskCheck(
            crate::error::RiskCheckError::MaxNotionalExceeded {
                current: open_notional,
                requested: legs_notional,
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
                    filled_qty(id) as filled_quantity, time_in_force, state, cancel_reason, \
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
                        filled_qty(id) as filled_quantity, time_in_force, state, cancel_reason, \
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

/// Check if an order with the given exchange_order_id exists in the database.
pub async fn order_exists(
    pool: &Pool,
    exchange_order_id: &str,
) -> Result<bool, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let row = client
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM prediction_orders WHERE exchange_order_id = $1) as exists",
            &[&exchange_order_id],
        )
        .await
        .map_err(|e| format!("order_exists: {}", e))?;

    Ok(row.get("exists"))
}

/// Record a settlement. Idempotent on (session_id, ticker).
/// Returns true if a new row was inserted, false if it already existed.
pub async fn record_settlement(
    pool: &Pool,
    session_id: i64,
    settlement: &crate::types::ExchangeSettlement,
) -> Result<bool, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let revenue_dollars = Decimal::new(settlement.revenue_cents, 2);
    let value_dollars = settlement.value_cents.map(|v| Decimal::new(v, 2));
    let market_result_str = settlement.market_result.to_string();

    let result = client
        .execute(
            "INSERT INTO settlements (session_id, ticker, event_ticker, market_result, yes_count, no_count, revenue_dollars, settled_time, fee_cost_dollars, value_dollars) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (session_id, ticker) DO NOTHING",
            &[
                &session_id,
                &settlement.ticker,
                &settlement.event_ticker,
                &market_result_str,
                &settlement.yes_count,
                &settlement.no_count,
                &revenue_dollars,
                &settlement.settled_time,
                &settlement.fee_cost_dollars,
                &value_dollars,
            ],
        )
        .await
        .map_err(|e| format!("record_settlement: {}", e))?;

    Ok(result > 0)
}

/// Get all tickers that have settlement records for a session.
pub async fn get_settled_tickers(
    pool: &Pool,
    session_id: i64,
) -> Result<std::collections::HashSet<String>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT ticker FROM settlements WHERE session_id = $1",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("get_settled_tickers: {}", e))?;

    Ok(rows.iter().map(|r| r.get("ticker")).collect())
}

/// Get all event tickers that have settlement records for a session.
///
/// Unlike `get_settled_tickers` which returns market-level tickers (where positions existed),
/// this returns event-level tickers. Useful for determining if a market settled even when
/// the user had no position in that specific market (e.g., unfilled resting orders).
pub async fn get_settled_event_tickers(
    pool: &Pool,
    session_id: i64,
) -> Result<std::collections::HashSet<String>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT DISTINCT event_ticker FROM settlements WHERE session_id = $1",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("get_settled_event_tickers: {}", e))?;

    Ok(rows.iter().map(|r| r.get("event_ticker")).collect())
}

/// Find all session IDs that have fills for a given ticker and no settlement recorded.
///
/// Used by the WS event ingester to decide whether a MarketSettled event is relevant
/// (i.e., we hold a position that needs settlement recording).
pub async fn sessions_with_unsettled_position(
    pool: &Pool,
    ticker: &str,
) -> Result<Vec<i64>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT DISTINCT session_id FROM fills f \
             JOIN prediction_orders o ON f.order_id = o.id \
             WHERE o.ticker = $1 \
               AND o.session_id NOT IN (SELECT session_id FROM settlements WHERE ticker = $1)",
            &[&ticker],
        )
        .await
        .map_err(|e| format!("sessions_with_unsettled_position: {}", e))?;

    Ok(rows.iter().map(|r| r.get("session_id")).collect())
}

/// List all settlements for a session.
pub async fn list_settlements(
    pool: &Pool,
    session_id: i64,
) -> Result<Vec<Settlement>, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let rows = client
        .query(
            "SELECT id, session_id, ticker, event_ticker, market_result, yes_count, no_count, \
                    revenue_dollars, settled_time, fee_cost_dollars, value_dollars, \
                    created_at \
             FROM settlements WHERE session_id = $1 ORDER BY settled_time DESC",
            &[&session_id],
        )
        .await
        .map_err(|e| format!("list_settlements: {}", e))?;

    let mut settlements = Vec::with_capacity(rows.len());
    for row in &rows {
        let market_result_str: &str = row.get("market_result");

        settlements.push(Settlement {
            id: row.get("id"),
            session_id: row.get("session_id"),
            ticker: row.get("ticker"),
            event_ticker: row.get("event_ticker"),
            market_result: market_result_str.parse().unwrap_or(MarketResult::Void),
            yes_count: row.get("yes_count"),
            no_count: row.get("no_count"),
            revenue_dollars: row.get("revenue_dollars"),
            settled_time: row.get("settled_time"),
            fee_cost_dollars: row.get("fee_cost_dollars"),
            value_dollars: row.get("value_dollars"),
            created_at: row.get("created_at"),
        });
    }

    Ok(settlements)
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

/// Cancel all staged siblings in the same order group when an order goes terminal.
///
/// When a root/entry order is cancelled by the exchange (e.g., market close),
/// the staged exit legs (TP, SL) must also transition to Cancelled. Without this,
/// staged orders linger and count toward open notional.
///
/// Also finalizes the group state if all legs are now terminal.
///
/// Returns the number of staged orders cancelled.
pub async fn cancel_staged_group_siblings(
    pool: &Pool,
    order_id: i64,
    session_id: i64,
) -> Result<u64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    // Find the group_id for this order
    let row = client
        .query_opt(
            "SELECT group_id FROM prediction_orders WHERE id = $1 AND session_id = $2",
            &[&order_id, &session_id],
        )
        .await
        .map_err(|e| format!("lookup group_id: {}", e))?;

    let group_id: Option<i64> = match row {
        Some(r) => r.get("group_id"),
        None => return Ok(0),
    };

    let group_id = match group_id {
        Some(gid) => gid,
        None => return Ok(0), // Not part of a group
    };

    // Cancel all staged siblings in the same group
    let cancelled = client
        .execute(
            "UPDATE prediction_orders SET state = 'cancelled', cancel_reason = 'exchange_cancel' \
             WHERE group_id = $1 AND session_id = $2 AND state = 'staged'",
            &[&group_id, &session_id],
        )
        .await
        .map_err(|e| format!("cancel staged siblings: {}", e))?;

    if cancelled > 0 {
        // Insert audit log entries for the cancelled staged orders
        client
            .execute(
                "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
                 SELECT id, 'staged', 'cancelled', 'parent_terminal', 'ws_event' \
                 FROM prediction_orders \
                 WHERE group_id = $1 AND session_id = $2 AND state = 'cancelled' AND cancel_reason = 'exchange_cancel'",
                &[&group_id, &session_id],
            )
            .await
            .map_err(|e| format!("audit staged cancels: {}", e))?;

        info!(group_id, cancelled, "cancelled staged group siblings");
    }

    // Finalize group if all legs are terminal
    let all_terminal_row = client
        .query_one(
            "SELECT COUNT(*) FILTER (WHERE state NOT IN ('filled', 'cancelled', 'rejected', 'expired')) AS open_count \
             FROM prediction_orders WHERE group_id = $1 AND session_id = $2",
            &[&group_id, &session_id],
        )
        .await
        .map_err(|e| format!("check group terminal: {}", e))?;

    let open_count: i64 = all_terminal_row.get("open_count");
    if open_count == 0 {
        let any_filled = client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM prediction_orders WHERE group_id = $1 AND session_id = $2 AND state = 'filled') AS has_fill",
                &[&group_id, &session_id],
            )
            .await
            .map_err(|e| format!("check group fills: {}", e))?;

        let final_state = if any_filled.get::<_, bool>("has_fill") {
            "completed"
        } else {
            "cancelled"
        };

        client
            .execute(
                "UPDATE order_groups SET state = $1 WHERE id = $2",
                &[&final_state, &group_id],
            )
            .await
            .map_err(|e| format!("finalize group: {}", e))?;

        info!(group_id, state = final_state, "group finalized after staged cancel");
    }

    Ok(cancelled)
}

/// Handle group state changes when an order fills.
///
/// Role-aware: if the filled order is a bracket entry, activates staged exit legs
/// (and returns the count so the caller can trigger a pump). If the filled order
/// is a bracket exit (TP/SL), cancels the other staged exit sibling(s) and
/// finalizes the group. For OCO, delegates to `cancel_staged_group_siblings`.
///
/// Returns the number of staged orders that were **activated** (needing a pump).
pub async fn handle_group_on_fill(
    pool: &Pool,
    order_id: i64,
    session_id: i64,
) -> Result<u64, String> {
    let client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    // Look up the filled order's group_id, leg_role, and group_type
    let row = client
        .query_opt(
            "SELECT o.group_id, o.leg_role, g.group_type \
             FROM prediction_orders o \
             LEFT JOIN order_groups g ON g.id = o.group_id \
             WHERE o.id = $1 AND o.session_id = $2",
            &[&order_id, &session_id],
        )
        .await
        .map_err(|e| format!("lookup group info: {}", e))?;

    let row = match row {
        Some(r) => r,
        None => return Ok(0),
    };

    let group_id: Option<i64> = row.get("group_id");
    let group_id = match group_id {
        Some(gid) => gid,
        None => return Ok(0), // Not part of a group
    };

    let leg_role: Option<String> = row.get("leg_role");
    let group_type: Option<String> = row.get("group_type");

    match (group_type.as_deref(), leg_role.as_deref()) {
        // Bracket entry filled → activate staged TP/SL exit legs
        (Some("bracket"), Some("entry")) => {
            let activated = activate_staged_siblings(pool, order_id, group_id, session_id).await?;
            Ok(activated)
        }
        // Bracket exit filled (TP or SL) → cancel the other exit leg(s)
        (Some("bracket"), Some("take_profit" | "stop_loss")) => {
            let _ = cancel_staged_group_siblings(pool, order_id, session_id).await?;
            Ok(0)
        }
        // OCO: cancel other legs (they're open, not staged)
        (Some("oco"), _) => {
            // OCO legs are both Pending/Acknowledged, not Staged.
            // cancel_staged_group_siblings handles staged; for open legs,
            // the evaluate_triggers path in groups.rs handles the cancel via EMS.
            // Here we just cancel any staged siblings (should be none for OCO).
            let _ = cancel_staged_group_siblings(pool, order_id, session_id).await?;
            Ok(0)
        }
        _ => {
            // Unknown group_type or leg_role — log and no-op
            warn!(order_id, group_id, ?leg_role, ?group_type, "unrecognized group/role on fill");
            Ok(0)
        }
    }
}

/// Activate all staged siblings of an order in the same group.
///
/// Transitions them from staged → pending and inserts queue items so they
/// get pumped to the exchange. Returns the number of activated orders.
async fn activate_staged_siblings(
    pool: &Pool,
    order_id: i64,
    group_id: i64,
    session_id: i64,
) -> Result<u64, String> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let tx = client
        .transaction()
        .await
        .map_err(|e| format!("begin tx: {}", e))?;

    // Find all staged siblings (same group, different order)
    let staged_rows = tx
        .query(
            "SELECT id FROM prediction_orders \
             WHERE group_id = $1 AND session_id = $2 AND state = 'staged' AND id != $3",
            &[&group_id, &session_id, &order_id],
        )
        .await
        .map_err(|e| format!("find staged siblings: {}", e))?;

    let mut activated = 0u64;
    for row in &staged_rows {
        let sibling_id: i64 = row.get("id");

        tx.execute(
            "UPDATE prediction_orders SET state = 'pending' \
             WHERE id = $1 AND session_id = $2 AND state = 'staged'",
            &[&sibling_id, &session_id],
        )
        .await
        .map_err(|e| format!("activate sibling {}: {}", sibling_id, e))?;

        tx.execute(
            "INSERT INTO order_queue (order_id, action, actor) VALUES ($1, 'submit', 'trigger')",
            &[&sibling_id],
        )
        .await
        .map_err(|e| format!("queue sibling {}: {}", sibling_id, e))?;

        tx.execute(
            "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
             VALUES ($1, 'staged', 'pending', 'activate', 'ws_fill_trigger')",
            &[&sibling_id],
        )
        .await
        .map_err(|e| format!("audit sibling {}: {}", sibling_id, e))?;

        activated += 1;
        debug!(order_id = sibling_id, group_id, "activated staged sibling on entry fill");
    }

    tx.commit()
        .await
        .map_err(|e| format!("commit: {}", e))?;

    if activated > 0 {
        info!(group_id, activated, "activated staged exit legs after entry fill");
    }

    Ok(activated)
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    use crate::state::OrderState;

    #[test]
    fn test_infer_event_submit() {
        let event = infer_event(OrderState::Pending, OrderState::Submitted).unwrap();
        assert_eq!(event.to_string(), "submit");
    }

    #[test]
    fn test_infer_event_cancel_from_pending() {
        let event = infer_event(OrderState::Pending, OrderState::Cancelled).unwrap();
        assert_eq!(event.to_string(), "cancel_request");
    }

    #[test]
    fn test_infer_event_cancel_confirm() {
        let event = infer_event(OrderState::PendingCancel, OrderState::Cancelled).unwrap();
        assert_eq!(event.to_string(), "cancel_confirm");
    }

    #[test]
    fn test_infer_event_activate() {
        let event = infer_event(OrderState::Staged, OrderState::Pending).unwrap();
        assert_eq!(event.to_string(), "activate");
    }

    #[test]
    fn test_validate_transition_valid() {
        assert!(validate_transition(OrderState::Pending, OrderState::Submitted).is_ok());
    }

    #[test]
    fn test_validate_transition_invalid() {
        assert!(validate_transition(OrderState::Pending, OrderState::Acknowledged).is_err());
    }

    #[test]
    fn test_validate_transition_cancel_from_pending() {
        let result = validate_transition(OrderState::Pending, OrderState::Cancelled);
        assert_eq!(result.unwrap(), OrderState::Cancelled);
    }
}
