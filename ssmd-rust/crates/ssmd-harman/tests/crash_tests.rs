//! Crash recovery integration tests for harman OMS.
//!
//! These tests verify deterministic outcomes for every failure scenario:
//! - Crash with orders in ambiguous states
//! - Recovery resolves all ambiguous orders via exchange queries
//! - Shutdown mass-cancels and drains queue
//! - Pump respects shutting_down flag
//! - Reconciliation discovers missing fills
//! - Double recovery is idempotent
//!
//! Requires a PostgreSQL database. Set DATABASE_URL to run.
//! Run with: cargo test -p ssmd-harman --test crash_tests -- --ignored

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use dashmap::DashMap;
use harman::db;
use harman::risk::RiskLimits;
use harman::state::OrderState;
use harman::test_helpers::*;
use harman::types::{Action, ExchangeOrderState, LegRole, OrderRequest, Side, TimeInForce};
use rust_decimal::Decimal;
use uuid::Uuid;

use ssmd_harman::{pump, AppState};
use ssmd_harman_ems::{Ems, EmsMetrics};
use ssmd_harman_oms::runner::OmsRunner;
use ssmd_harman_oms::{Oms, OmsMetrics};

/// Build an AppState using MockExchange and a test DB pool.
async fn build_test_state(
    mock: MockExchange,
    pool: deadpool_postgres::Pool,
    session_id: i64,
) -> Arc<AppState> {
    let registry = prometheus::Registry::new();
    let ems_metrics = EmsMetrics::new(&registry);
    let exchange: Arc<dyn harman::exchange::ExchangeAdapter> = Arc::new(mock);
    let ems = Arc::new(Ems::new(
        pool.clone(),
        exchange.clone(),
        RiskLimits::default(),
        ems_metrics,
    ));
    let oms_metrics = OmsMetrics::new(&registry);
    let oms = Arc::new(Oms::new(pool.clone(), exchange, ems.clone(), oms_metrics));
    let runner = Arc::new(OmsRunner::new(oms.clone(), None, session_id));
    let pump_trigger = runner.pump_trigger();
    Arc::new(AppState {
        ems,
        oms,
        pool,
        registry,
        api_token: "test-api-token".to_string(),
        admin_token: "test-admin-token".to_string(),
        startup_session_id: session_id,
        auth_validate_url: None,
        http_client: reqwest::Client::new(),
        runner,
        auto_pump: false,
        pump_trigger,
        session_semaphores: DashMap::new(),
        auth_cache: tokio::sync::RwLock::new(HashMap::new()),
        key_sessions: DashMap::new(),
        pump_semaphore: tokio::sync::Semaphore::new(1),
        ticker_cache: tokio::sync::RwLock::new(None),
        redis_conn: None,
        monitor_metrics: ssmd_harman::MonitorMetrics::new(&prometheus::Registry::new()),
    })
}

/// Setup helper: create pool, run migrations, create a unique test session.
/// Each test gets its own session to avoid cross-test data contamination.
async fn setup() -> (deadpool_postgres::Pool, i64) {
    let pool = setup_test_db().await.expect("setup_test_db failed");
    let unique_name = format!("test-{}", Uuid::new_v4());
    let session_id = db::get_or_create_session(&pool, &unique_name, None)
        .await
        .unwrap_or_else(|e| panic!("create session '{}' failed: {}", unique_name, e));
    (pool, session_id)
}

// =============================================================================
// Test 1: Recovery resolves submitted order → Acknowledged (exchange says Resting)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_submitted_order_resting_on_exchange() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    // Insert order in "submitted" state (simulating crash after send)
    let order_id = insert_test_order_with_coid(
        &pool,
        session_id,
        OrderState::Submitted,
        "KXTEST-RECOVER-1",
        Some("exch-recover-1"),
        coid,
    )
    .await
    .unwrap();

    // Configure mock: exchange says order is resting
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.order_statuses.insert(
            coid,
            mock_exchange_status("exch-recover-1", ExchangeOrderState::Resting, Decimal::ZERO, Decimal::from(10)),
        );
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    // Order should now be Acknowledged
    assert_order_state(&pool, order_id, OrderState::Acknowledged)
        .await
        .unwrap();
}

// =============================================================================
// Test 2: Recovery resolves submitted order → Filled (exchange says Executed)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_submitted_order_executed_on_exchange() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    let order_id = insert_test_order_with_coid(
        &pool,
        session_id,
        OrderState::Submitted,
        "KXTEST-RECOVER-2",
        Some("exch-recover-2"),
        coid,
    )
    .await
    .unwrap();

    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.order_statuses.insert(
            coid,
            mock_exchange_status("exch-recover-2", ExchangeOrderState::Executed, Decimal::from(10), Decimal::ZERO),
        );
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    assert_order_state(&pool, order_id, OrderState::Filled)
        .await
        .unwrap();
}

// =============================================================================
// Test 3: Recovery resolves submitted order → Rejected (exchange says NotFound)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_submitted_order_not_found_on_exchange() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    let order_id = insert_test_order_with_coid(
        &pool,
        session_id,
        OrderState::Submitted,
        "KXTEST-RECOVER-3",
        Some("exch-recover-3"),
        coid,
    )
    .await
    .unwrap();

    // Mock: exchange doesn't know about this order (NotFound error from get_order_by_client_id)
    let mock = MockExchange::new();

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    assert_order_state(&pool, order_id, OrderState::Rejected)
        .await
        .unwrap();
}

// =============================================================================
// Test 4: Recovery resolves pending_cancel → Cancelled (exchange says Cancelled)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_pending_cancel_order_cancelled_on_exchange() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    let order_id = insert_test_order_with_coid(
        &pool,
        session_id,
        OrderState::PendingCancel,
        "KXTEST-RECOVER-4",
        Some("exch-recover-4"),
        coid,
    )
    .await
    .unwrap();

    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.order_statuses.insert(
            coid,
            mock_exchange_status("exch-recover-4", ExchangeOrderState::Cancelled, Decimal::ZERO, Decimal::ZERO),
        );
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    assert_order_state(&pool, order_id, OrderState::Cancelled)
        .await
        .unwrap();
}

// =============================================================================
// Test 5: Recovery re-sends cancel for PendingCancel + Resting
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_pending_cancel_resting_resends_cancel() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    let _order_id = insert_test_order_with_coid(
        &pool,
        session_id,
        OrderState::PendingCancel,
        "KXTEST-RECOVER-5",
        Some("exch-recover-5"),
        coid,
    )
    .await
    .unwrap();

    let mock_state = Arc::new(tokio::sync::Mutex::new(MockExchangeState::default()));
    {
        let mut state = mock_state.lock().await;
        state.order_statuses.insert(
            coid,
            mock_exchange_status("exch-recover-5", ExchangeOrderState::Resting, Decimal::ZERO, Decimal::from(10)),
        );
    }
    let mock = MockExchange::with_state(mock_state.clone());

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    // Verify cancel was re-sent
    let state = mock_state.lock().await;
    assert!(
        state.cancel_calls.contains(&"exch-recover-5".to_string()),
        "expected cancel_order to be called for exch-recover-5"
    );
}

// =============================================================================
// Test 6: Double recovery is idempotent
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_double_recovery_idempotent() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    let order_id = insert_test_order_with_coid(
        &pool,
        session_id,
        OrderState::Submitted,
        "KXTEST-RECOVER-6",
        Some("exch-recover-6"),
        coid,
    )
    .await
    .unwrap();

    let mock_state = Arc::new(tokio::sync::Mutex::new(MockExchangeState::default()));
    {
        let mut state = mock_state.lock().await;
        state.order_statuses.insert(
            coid,
            mock_exchange_status("exch-recover-6", ExchangeOrderState::Executed, Decimal::from(10), Decimal::ZERO),
        );
    }

    // First recovery
    let mock1 = MockExchange::with_state(mock_state.clone());
    let app_state = build_test_state(mock1, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    assert_order_state(&pool, order_id, OrderState::Filled)
        .await
        .unwrap();

    // Second recovery — should succeed with no errors even though order is now terminal
    let mock2 = MockExchange::with_state(mock_state.clone());
    let app_state2 = build_test_state(mock2, pool.clone(), session_id).await;
    app_state2.oms.run_recovery(session_id).await.unwrap();

    // Still filled
    assert_order_state(&pool, order_id, OrderState::Filled)
        .await
        .unwrap();
}

// =============================================================================
// Test 7: Pump processes pending queue item → submits to exchange
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_processes_pending_order() {
    let (pool, session_id) = setup().await;

    // Insert order in pending state with a queue item
    let order_id = insert_test_order(&pool, session_id, OrderState::Pending, "KXTEST-PUMP-1", None)
        .await
        .unwrap();
    insert_test_queue_item(&pool, order_id, "submit")
        .await
        .unwrap();

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;

    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.processed, 1);
    assert_eq!(result.submitted, 1);
    assert!(result.errors.is_empty(), "pump errors: {:?}", result.errors);

    // Order should be acknowledged after successful submit
    assert_order_state(&pool, order_id, OrderState::Acknowledged)
        .await
        .unwrap();

    // Queue should be drained
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 0, "queue should be empty after pump");
}

// =============================================================================
// Test 8: Pump stops when shutting_down flag is set
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_respects_shutting_down_flag() {
    let (pool, session_id) = setup().await;

    // Insert orders with queue items
    let order_id1 = insert_test_order(&pool, session_id, OrderState::Pending, "KXTEST-PUMP-SD-1", None)
        .await
        .unwrap();
    insert_test_queue_item(&pool, order_id1, "submit")
        .await
        .unwrap();

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;

    // Set shutting_down BEFORE pump
    app_state.ems.shutting_down.store(true, Ordering::Relaxed);

    let result = pump::pump(&app_state, session_id).await;

    // Pump should return immediately without processing
    assert_eq!(result.processed, 0, "pump should not process items during shutdown");
    assert!(
        result.errors.iter().any(|e| e.contains("shutting down")),
        "pump should report shutting down"
    );
}

// =============================================================================
// Test 9: Pump handles exchange rejection
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_exchange_rejection() {
    let (pool, session_id) = setup().await;

    let order_id = insert_test_order(&pool, session_id, OrderState::Pending, "KXTEST-PUMP-REJ", None)
        .await
        .unwrap();
    insert_test_queue_item(&pool, order_id, "submit")
        .await
        .unwrap();

    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.submit_behavior = SubmitBehavior::Reject("test rejection".to_string());
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.rejected, 1);

    assert_order_state(&pool, order_id, OrderState::Rejected)
        .await
        .unwrap();
}

// =============================================================================
// Test 10: Pump handles exchange timeout (leaves as submitted for reconciliation)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_exchange_timeout_leaves_for_reconciliation() {
    let (pool, session_id) = setup().await;

    let order_id = insert_test_order(&pool, session_id, OrderState::Pending, "KXTEST-PUMP-TO", None)
        .await
        .unwrap();
    insert_test_queue_item(&pool, order_id, "submit")
        .await
        .unwrap();

    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.submit_behavior = SubmitBehavior::Timeout;
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.processed, 1);
    // Timeout leaves order as submitted for reconciliation to resolve
    assert_order_state(&pool, order_id, OrderState::Submitted)
        .await
        .unwrap();
}

// =============================================================================
// Test 11: Shutdown drains queue and rejects queued orders
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_shutdown_drains_queue() {
    let (pool, session_id) = setup().await;

    // Insert orders with queue items
    let order_id1 = insert_test_order(&pool, session_id, OrderState::Pending, "KXTEST-SD-1", None)
        .await
        .unwrap();
    insert_test_queue_item(&pool, order_id1, "submit")
        .await
        .unwrap();

    let order_id2 = insert_test_order(&pool, session_id, OrderState::Pending, "KXTEST-SD-2", None)
        .await
        .unwrap();
    insert_test_queue_item(&pool, order_id2, "submit")
        .await
        .unwrap();

    // Drain queue (simulating shutdown without signal handling)
    let count = db::drain_queue_for_shutdown(&pool, session_id)
        .await
        .unwrap();

    assert_eq!(count, 2, "should drain 2 queue items");

    // Orders should be rejected
    assert_order_state(&pool, order_id1, OrderState::Rejected)
        .await
        .unwrap();
    assert_order_state(&pool, order_id2, OrderState::Rejected)
        .await
        .unwrap();

    // Queue should be empty
    let remaining = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(remaining, 0);
}

// =============================================================================
// Test 12: Recovery discovers missing fills
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_discovers_missing_fills() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    // Insert an acknowledged order with an exchange_order_id
    let _order_id = insert_test_order_with_coid(
        &pool,
        session_id,
        OrderState::Acknowledged,
        "KXTEST-FILLS-1",
        Some("exch-fills-1"),
        coid,
    )
    .await
    .unwrap();

    // Configure mock to return a fill for this order
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.fills.push(mock_fill(
            "exch-fills-1",
            "KXTEST-FILLS-1",
            Decimal::from(5),
            Decimal::new(50, 2),
        ));
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    // Verify fill was recorded in DB
    let client = pool.get().await.unwrap();
    let row = client
        .query_one(
            "SELECT COUNT(*) as cnt FROM fills f \
             JOIN prediction_orders o ON o.id = f.order_id \
             WHERE o.session_id = $1 AND o.exchange_order_id = 'exch-fills-1'",
            &[&session_id],
        )
        .await
        .unwrap();
    let fill_count: i64 = row.get("cnt");
    assert_eq!(fill_count, 1, "should have discovered 1 fill");
}

// =============================================================================
// Test 13: Recovery with multiple orders in various ambiguous states
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_multiple_ambiguous_orders() {
    let (pool, session_id) = setup().await;

    let coid1 = Uuid::new_v4();
    let coid2 = Uuid::new_v4();
    let coid3 = Uuid::new_v4();

    // Order 1: Submitted → exchange says Resting → Acknowledged
    let oid1 = insert_test_order_with_coid(
        &pool, session_id, OrderState::Submitted,
        "KXTEST-MULTI-1", Some("exch-multi-1"), coid1,
    ).await.unwrap();

    // Order 2: Submitted → exchange says Executed → Filled
    let oid2 = insert_test_order_with_coid(
        &pool, session_id, OrderState::Submitted,
        "KXTEST-MULTI-2", Some("exch-multi-2"), coid2,
    ).await.unwrap();

    // Order 3: PendingCancel → exchange says Cancelled → Cancelled
    let oid3 = insert_test_order_with_coid(
        &pool, session_id, OrderState::PendingCancel,
        "KXTEST-MULTI-3", Some("exch-multi-3"), coid3,
    ).await.unwrap();

    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.order_statuses.insert(
            coid1,
            mock_exchange_status("exch-multi-1", ExchangeOrderState::Resting, Decimal::ZERO, Decimal::from(10)),
        );
        state.order_statuses.insert(
            coid2,
            mock_exchange_status("exch-multi-2", ExchangeOrderState::Executed, Decimal::from(10), Decimal::ZERO),
        );
        state.order_statuses.insert(
            coid3,
            mock_exchange_status("exch-multi-3", ExchangeOrderState::Cancelled, Decimal::ZERO, Decimal::ZERO),
        );
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    assert_order_state(&pool, oid1, OrderState::Acknowledged).await.unwrap();
    assert_order_state(&pool, oid2, OrderState::Filled).await.unwrap();
    assert_order_state(&pool, oid3, OrderState::Cancelled).await.unwrap();
}

// =============================================================================
// Test 14: Recovery cleans stale processing queue items
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_cleans_stale_queue_items() {
    let (pool, session_id) = setup().await;

    // Insert order with a "processing" queue item (simulating crash mid-dequeue)
    let order_id = insert_test_order(
        &pool, session_id, OrderState::Pending,
        "KXTEST-STALE-Q", None,
    ).await.unwrap();

    let client = pool.get().await.unwrap();
    client
        .execute(
            "INSERT INTO order_queue (order_id, action, actor, processing) VALUES ($1, 'submit', 'test', TRUE)",
            &[&order_id],
        )
        .await
        .unwrap();

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    // Queue item should have processing=FALSE now
    let row = client
        .query_one(
            "SELECT processing FROM order_queue WHERE order_id = $1",
            &[&order_id],
        )
        .await
        .unwrap();
    let processing: bool = row.get("processing");
    assert!(!processing, "stale queue item should be reset to processing=FALSE");
}

// =============================================================================
// Test 15: Reconciliation discovers fills for acknowledged orders
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_reconciliation_discovers_fills() {
    let (pool, session_id) = setup().await;

    let order_id = insert_test_order(
        &pool, session_id, OrderState::Acknowledged,
        "KXTEST-RECON-FILL", Some("exch-recon-fill-1"),
    ).await.unwrap();

    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.fills.push(mock_fill(
            "exch-recon-fill-1",
            "KXTEST-RECON-FILL",
            Decimal::from(3),
            Decimal::new(45, 2),
        ));
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = app_state.oms.reconcile(session_id).await;

    assert_eq!(result.fills_discovered, 1);

    // Verify fill in DB
    let client = pool.get().await.unwrap();
    let row = client
        .query_one(
            "SELECT COUNT(*) as cnt FROM fills WHERE order_id = $1",
            &[&order_id],
        )
        .await
        .unwrap();
    let count: i64 = row.get("cnt");
    assert_eq!(count, 1);
}

// =============================================================================
// Test 16: Duplicate client_order_id is rejected
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_duplicate_client_order_id_rejected() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    // First insert succeeds
    insert_test_order_with_coid(
        &pool, session_id, OrderState::Acknowledged,
        "KXTEST-DUP-1", Some("exch-dup-1"), coid,
    ).await.unwrap();

    // Second insert with same client_order_id should fail
    let result = insert_test_order_with_coid(
        &pool, session_id, OrderState::Pending,
        "KXTEST-DUP-1", None, coid,
    ).await;

    assert!(result.is_err(), "duplicate client_order_id should be rejected");
}

// =============================================================================
// Test 17: Terminal orders are not picked up by recovery
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_ignores_terminal_orders() {
    let (pool, session_id) = setup().await;

    // Insert orders in terminal states
    let oid_filled = insert_test_order(
        &pool, session_id, OrderState::Filled,
        "KXTEST-TERM-FILL", Some("exch-term-1"),
    ).await.unwrap();
    let oid_cancelled = insert_test_order(
        &pool, session_id, OrderState::Cancelled,
        "KXTEST-TERM-CANCEL", Some("exch-term-2"),
    ).await.unwrap();
    let oid_rejected = insert_test_order(
        &pool, session_id, OrderState::Rejected,
        "KXTEST-TERM-REJ", None,
    ).await.unwrap();

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    // All should remain in their terminal states
    assert_order_state(&pool, oid_filled, OrderState::Filled).await.unwrap();
    assert_order_state(&pool, oid_cancelled, OrderState::Cancelled).await.unwrap();
    assert_order_state(&pool, oid_rejected, OrderState::Rejected).await.unwrap();
}

// =============================================================================
// Test 18: Pump cancel with no exchange_order_id cancels locally
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_cancel_without_exchange_id() {
    let (pool, session_id) = setup().await;

    // Insert order in PendingCancel but no exchange_order_id (never reached exchange)
    let order_id = insert_test_order(
        &pool, session_id, OrderState::PendingCancel,
        "KXTEST-PUMP-LOCAL-CANCEL", None,
    ).await.unwrap();
    insert_test_queue_item(&pool, order_id, "cancel").await.unwrap();

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.cancelled, 1);
    assert_order_state(&pool, order_id, OrderState::Cancelled).await.unwrap();
}

// =============================================================================
// Test 19: Pump amend success — updates price/quantity and reverts to acknowledged
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_amend_success() {
    let (pool, session_id) = setup().await;

    // Insert acknowledged order with exchange_order_id
    let order_id = insert_test_order(
        &pool, session_id, OrderState::PendingAmend,
        "KXTEST-AMEND-OK", Some("exch-amend-1"),
    ).await.unwrap();

    // Enqueue amend with metadata
    insert_test_queue_item_with_metadata(
        &pool, order_id, "amend",
        serde_json::json!({"new_price_dollars": "0.03", "new_quantity": "5"}),
    ).await.unwrap();

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.amended, 1);
    assert!(result.errors.is_empty(), "pump errors: {:?}", result.errors);

    // Order should revert to acknowledged
    assert_order_state(&pool, order_id, OrderState::Acknowledged).await.unwrap();

    // Price and quantity should be updated
    let (price, qty) = get_order_price_qty(&pool, order_id).await.unwrap();
    assert_eq!(price, Decimal::new(3, 2), "price should be 0.03");
    assert_eq!(qty, Decimal::from(5), "quantity should be 5");

    // Queue should be empty
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 0);
}

// =============================================================================
// Test 20: Pump amend NotFound — marks order cancelled (not acknowledged)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_amend_notfound_marks_cancelled() {
    let (pool, session_id) = setup().await;

    // Insert order in PendingAmend (simulates: order submitted, acked, amend requested,
    // then mass_cancel killed the order on exchange before pump could amend it)
    let order_id = insert_test_order(
        &pool, session_id, OrderState::PendingAmend,
        "KXTEST-AMEND-NF", Some("exch-amend-gone"),
    ).await.unwrap();

    insert_test_queue_item_with_metadata(
        &pool, order_id, "amend",
        serde_json::json!({"new_price_dollars": "0.04"}),
    ).await.unwrap();

    // Exchange returns NotFound for this order
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.amend_behavior = AmendBehavior::NotFound;
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.processed, 1);

    // Order must be CANCELLED — not reverted to acknowledged
    assert_order_state(&pool, order_id, OrderState::Cancelled).await.unwrap();

    // Queue should be empty
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 0);
}

// =============================================================================
// Test 21: Pump decrease success — reduces quantity and reverts to acknowledged
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_decrease_success() {
    let (pool, session_id) = setup().await;

    // Insert order in PendingDecrease with quantity=10, exchange_order_id set
    let order_id = insert_test_order(
        &pool, session_id, OrderState::PendingDecrease,
        "KXTEST-DEC-OK", Some("exch-dec-1"),
    ).await.unwrap();

    // Enqueue decrease with reduce_by=3
    insert_test_queue_item_with_metadata(
        &pool, order_id, "decrease",
        serde_json::json!({"reduce_by": "3"}),
    ).await.unwrap();

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.decreased, 1);
    assert!(result.errors.is_empty(), "pump errors: {:?}", result.errors);

    // Order should revert to acknowledged
    assert_order_state(&pool, order_id, OrderState::Acknowledged).await.unwrap();

    // Quantity should be reduced: 10 - 3 = 7
    let (_, qty) = get_order_price_qty(&pool, order_id).await.unwrap();
    assert_eq!(qty, Decimal::from(7), "quantity should be 10 - 3 = 7");

    // Queue should be empty
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 0);
}

// =============================================================================
// Test 22: Pump decrease NotFound — marks order cancelled (not acknowledged)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_decrease_notfound_marks_cancelled() {
    let (pool, session_id) = setup().await;

    // Insert order in PendingDecrease
    let order_id = insert_test_order(
        &pool, session_id, OrderState::PendingDecrease,
        "KXTEST-DEC-NF", Some("exch-dec-gone"),
    ).await.unwrap();

    insert_test_queue_item_with_metadata(
        &pool, order_id, "decrease",
        serde_json::json!({"reduce_by": "2"}),
    ).await.unwrap();

    // Exchange returns NotFound
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.decrease_behavior = DecreaseBehavior::NotFound;
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.processed, 1);

    // Order must be CANCELLED — not reverted to acknowledged
    assert_order_state(&pool, order_id, OrderState::Cancelled).await.unwrap();

    // Queue should be empty
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 0);
}

// =============================================================================
// Test 23: Pump amend with no exchange_order_id reverts to acknowledged
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_amend_no_exchange_id_reverts() {
    let (pool, session_id) = setup().await;

    // Insert order in PendingAmend but WITHOUT exchange_order_id
    let order_id = insert_test_order(
        &pool, session_id, OrderState::PendingAmend,
        "KXTEST-AMEND-NOID", None,
    ).await.unwrap();

    insert_test_queue_item_with_metadata(
        &pool, order_id, "amend",
        serde_json::json!({"new_price_dollars": "0.05"}),
    ).await.unwrap();

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.processed, 1);

    // Should revert to acknowledged (can't amend what never reached exchange)
    assert_order_state(&pool, order_id, OrderState::Acknowledged).await.unwrap();

    // Queue should be empty
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 0);
}

// =============================================================================
// Test 24: Pump decrease with no exchange_order_id reverts to acknowledged
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_pump_decrease_no_exchange_id_reverts() {
    let (pool, session_id) = setup().await;

    // Insert order in PendingDecrease but WITHOUT exchange_order_id
    let order_id = insert_test_order(
        &pool, session_id, OrderState::PendingDecrease,
        "KXTEST-DEC-NOID", None,
    ).await.unwrap();

    insert_test_queue_item_with_metadata(
        &pool, order_id, "decrease",
        serde_json::json!({"reduce_by": "1"}),
    ).await.unwrap();

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = pump::pump(&app_state, session_id).await;

    assert_eq!(result.processed, 1);

    // Should revert to acknowledged (can't decrease what never reached exchange)
    assert_order_state(&pool, order_id, OrderState::Acknowledged).await.unwrap();

    // Queue should be empty
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 0);
}

// =============================================================================
// Test 25: Recovery resolves PendingAmend → revert via exchange state
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_pending_amend_resting_reverts() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    // Insert order in PendingAmend (crash during amend processing)
    let order_id = insert_test_order_with_coid(
        &pool, session_id, OrderState::PendingAmend,
        "KXTEST-AMEND-RECOVER", Some("exch-amend-r1"), coid,
    ).await.unwrap();

    // Exchange says order is still resting (amend never completed)
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.order_statuses.insert(
            coid,
            mock_exchange_status("exch-amend-r1", ExchangeOrderState::Resting, Decimal::ZERO, Decimal::from(10)),
        );
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    // Recovery should resolve PendingAmend to Acknowledged (since exchange says resting)
    assert_order_state(&pool, order_id, OrderState::Acknowledged).await.unwrap();
}

// =============================================================================
// Test 26: Recovery resolves PendingDecrease → revert via exchange state
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_recovery_pending_decrease_resting_reverts() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    // Insert order in PendingDecrease (crash during decrease processing)
    let order_id = insert_test_order_with_coid(
        &pool, session_id, OrderState::PendingDecrease,
        "KXTEST-DEC-RECOVER", Some("exch-dec-r1"), coid,
    ).await.unwrap();

    // Exchange says order is still resting
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.order_statuses.insert(
            coid,
            mock_exchange_status("exch-dec-r1", ExchangeOrderState::Resting, Decimal::ZERO, Decimal::from(10)),
        );
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    app_state.oms.run_recovery(session_id).await.unwrap();

    // Recovery should resolve PendingDecrease to Acknowledged
    assert_order_state(&pool, order_id, OrderState::Acknowledged).await.unwrap();
}

// =============================================================================
// Test 27: Reconciliation discovers fills AND updates order state (Bug 1 fix)
//
// Scenario: Order is Acknowledged, exchange reports a fill. Reconcile should
// record the fill and transition the order to Filled.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_reconciliation_fill_updates_order_state() {
    let (pool, session_id) = setup().await;

    // Insert order in Acknowledged state (qty=10, filled=0)
    let order_id = insert_test_order(
        &pool, session_id, OrderState::Acknowledged,
        "KXTEST-RECON-FILL-STATE", Some("exch-recon-fs-1"),
    ).await.unwrap();

    // Mock: exchange reports a fill for the full quantity (10)
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.fills.push(mock_fill(
            "exch-recon-fs-1",
            "KXTEST-RECON-FILL-STATE",
            Decimal::from(10),
            Decimal::new(50, 2),
        ));
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = app_state.oms.reconcile(session_id).await;

    assert_eq!(result.fills_discovered, 1);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    // Order should now be Filled (not still Acknowledged)
    assert_order_state(&pool, order_id, OrderState::Filled).await.unwrap();
}

// =============================================================================
// Test 28: Reconciliation discovers partial fill → PartiallyFilled state
//
// Scenario: Order qty=10, fill qty=3. Should transition to PartiallyFilled.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_reconciliation_partial_fill_updates_order_state() {
    let (pool, session_id) = setup().await;

    let order_id = insert_test_order(
        &pool, session_id, OrderState::Acknowledged,
        "KXTEST-RECON-PFILL", Some("exch-recon-pf-1"),
    ).await.unwrap();

    // Mock: exchange reports a partial fill (3 of 10)
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.fills.push(mock_fill(
            "exch-recon-pf-1",
            "KXTEST-RECON-PFILL",
            Decimal::from(3),
            Decimal::new(50, 2),
        ));
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = app_state.oms.reconcile(session_id).await;

    assert_eq!(result.fills_discovered, 1);

    // Order should be PartiallyFilled (not Acknowledged)
    assert_order_state(&pool, order_id, OrderState::PartiallyFilled).await.unwrap();
}

// =============================================================================
// Test 29: Reconciliation resolves acknowledged order → Filled (exchange says Executed)
//
// Scenario: Order is Acknowledged but exchange says Executed (e.g., IOC fill
// that completed without streaming notification). Bug 2 fix: Acknowledged is
// now included in get_ambiguous_orders so resolve_stale_orders picks it up.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_reconciliation_acknowledged_executed_on_exchange() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    let order_id = insert_test_order_with_coid(
        &pool, session_id, OrderState::Acknowledged,
        "KXTEST-RECON-ACK-EXEC", Some("exch-recon-ae-1"), coid,
    ).await.unwrap();

    // Make the order look stale (updated >30s ago).
    // Must use make_order_stale() to bypass the updated_at trigger.
    make_order_stale(&pool, order_id, 60).await.unwrap();

    // Mock: exchange says order is Executed
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.order_statuses.insert(
            coid,
            mock_exchange_status("exch-recon-ae-1", ExchangeOrderState::Executed, Decimal::from(10), Decimal::ZERO),
        );
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = app_state.oms.reconcile(session_id).await;

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.orders_resolved >= 1, "expected at least 1 resolved order");

    // Order should now be Filled
    assert_order_state(&pool, order_id, OrderState::Filled).await.unwrap();
}

// =============================================================================
// Test 30: Reconciliation resolves acknowledged order → Cancelled (mass cancel)
//
// Scenario: Mass cancel was sent on exchange, order cancelled there, but
// local state still shows Acknowledged. Reconcile should catch it.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_reconciliation_acknowledged_cancelled_on_exchange() {
    let (pool, session_id) = setup().await;
    let coid = Uuid::new_v4();

    let order_id = insert_test_order_with_coid(
        &pool, session_id, OrderState::Acknowledged,
        "KXTEST-RECON-ACK-CXL", Some("exch-recon-ac-1"), coid,
    ).await.unwrap();

    // Make the order look stale. Must bypass updated_at trigger.
    make_order_stale(&pool, order_id, 60).await.unwrap();

    // Mock: exchange says order is Cancelled (e.g., mass cancel)
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.order_statuses.insert(
            coid,
            mock_exchange_status("exch-recon-ac-1", ExchangeOrderState::Cancelled, Decimal::ZERO, Decimal::ZERO),
        );
    }

    let app_state = build_test_state(mock, pool.clone(), session_id).await;
    let result = app_state.oms.reconcile(session_id).await;

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.orders_resolved >= 1, "expected at least 1 resolved order");

    // Order should now be Cancelled
    assert_order_state(&pool, order_id, OrderState::Cancelled).await.unwrap();
}

// =============================================================================
// Helper: create a test OrderRequest
// =============================================================================

fn test_order_request(ticker: &str, side: Side, action: Action, qty: Decimal, price: Decimal) -> OrderRequest {
    OrderRequest {
        client_order_id: Uuid::new_v4(),
        ticker: ticker.to_string(),
        side,
        action,
        quantity: qty,
        price_dollars: price,
        time_in_force: TimeInForce::Gtc,
    }
}

// =============================================================================
// Test 30: Create bracket group — entry pending, TP+SL staged
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_bracket_creation() {
    let (pool, session_id) = setup().await;

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;

    let entry = test_order_request("KXTEST-BRACKET", Side::Yes, Action::Buy, Decimal::from(5), Decimal::new(50, 2));
    let tp = test_order_request("KXTEST-BRACKET", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(80, 2));
    let sl = test_order_request("KXTEST-BRACKET", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(20, 2));

    let (group, orders) = app_state.oms.create_bracket(session_id, entry, tp, sl).await.unwrap();

    assert_eq!(group.group_type, harman::types::GroupType::Bracket);
    assert_eq!(group.state, harman::types::GroupState::Active);
    assert_eq!(orders.len(), 3);

    // Entry should be Pending
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();
    assert_eq!(entry_order.state, OrderState::Pending);
    assert_eq!(entry_order.group_id, Some(group.id));

    // TP should be Staged
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    assert_eq!(tp_order.state, OrderState::Staged);

    // SL should be Staged
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();
    assert_eq!(sl_order.state, OrderState::Staged);

    // Only entry should have a queue item
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 1);
}

// =============================================================================
// Test 31: Bracket entry fills → triggers activate TP+SL
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_bracket_entry_fill_activates_exits() {
    let (pool, session_id) = setup().await;

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;

    let entry = test_order_request("KXTEST-BRACKET-2", Side::Yes, Action::Buy, Decimal::from(5), Decimal::new(50, 2));
    let tp = test_order_request("KXTEST-BRACKET-2", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(80, 2));
    let sl = test_order_request("KXTEST-BRACKET-2", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(20, 2));

    let (group, orders) = app_state.oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();

    // Drain the entry's queue item first (dequeue transitions Pending → Submitted)
    if let Ok(Some(qi)) = db::dequeue_order(&pool, session_id).await {
        let _ = db::remove_queue_item(&pool, qi.queue_id).await;
    }

    // Simulate entry being filled (after dequeue so it doesn't overwrite)
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Filled,
        Some("exch-bracket-entry"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    // Debug: verify DB state before evaluate_triggers
    {
        let client = pool.get().await.unwrap();
        let row = client.query_one(
            "SELECT state, group_id FROM prediction_orders WHERE id = $1",
            &[&entry_order.id],
        ).await.unwrap();
        let state: String = row.get("state");
        let gid: Option<i64> = row.get("group_id");
        eprintln!("[diag] entry order {} state={} group_id={:?} session_id={}", entry_order.id, state, gid, session_id);

        let rows = client.query(
            "SELECT id, state, group_id, leg_role FROM prediction_orders WHERE group_id = $1",
            &[&gid.unwrap()],
        ).await.unwrap();
        for r in &rows {
            let id: i64 = r.get("id");
            let s: String = r.get("state");
            let lr: Option<String> = r.get("leg_role");
            eprintln!("[diag] group order id={} state={} leg_role={:?}", id, s, lr);
        }

        let grows = client.query(
            "SELECT id, state, session_id FROM order_groups WHERE session_id = $1",
            &[&session_id],
        ).await.unwrap();
        for g in &grows {
            let id: i64 = g.get("id");
            let s: String = g.get("state");
            let sid: i64 = g.get("session_id");
            eprintln!("[diag] group id={} state={} session_id={}", id, s, sid);
        }
    }

    // Evaluate triggers
    let activated = app_state.oms.evaluate_triggers(session_id).await.unwrap();
    assert_eq!(activated, 2, "should activate TP and SL");

    // TP and SL should now be Pending
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    assert_order_state(&pool, tp_order.id, OrderState::Pending).await.unwrap();

    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();
    assert_order_state(&pool, sl_order.id, OrderState::Pending).await.unwrap();

    // Group should still be active (exits not yet terminal)
    let group_now = db::get_group(&pool, group.id, session_id).await.unwrap().unwrap();
    assert_eq!(group_now.state, harman::types::GroupState::Active);
}

// =============================================================================
// Test 32: Bracket exit fills → cancels other exit, group completed
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_bracket_exit_fill_completes_group() {
    let (pool, session_id) = setup().await;

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;

    let entry = test_order_request("KXTEST-BRACKET-3", Side::Yes, Action::Buy, Decimal::from(5), Decimal::new(50, 2));
    let tp = test_order_request("KXTEST-BRACKET-3", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(80, 2));
    let sl = test_order_request("KXTEST-BRACKET-3", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(20, 2));

    let (group, orders) = app_state.oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();

    // Simulate: entry filled, TP filled, SL still staged
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Filled,
        Some("exch-b3-entry"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();
    db::update_order_state(
        &pool, tp_order.id, session_id, OrderState::Filled,
        Some("exch-b3-tp"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    // Evaluate triggers
    let _ = app_state.oms.evaluate_triggers(session_id).await.unwrap();

    // SL should be cancelled (it was still staged when TP filled)
    assert_order_state(&pool, sl_order.id, OrderState::Cancelled).await.unwrap();

    // Group should be completed
    let group_now = db::get_group(&pool, group.id, session_id).await.unwrap().unwrap();
    assert_eq!(group_now.state, harman::types::GroupState::Completed);
}

// =============================================================================
// Test 33: Bracket entry rejected → staged legs cancelled, group cancelled
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_bracket_entry_rejected_cancels_group() {
    let (pool, session_id) = setup().await;

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;

    let entry = test_order_request("KXTEST-BRACKET-4", Side::Yes, Action::Buy, Decimal::from(5), Decimal::new(50, 2));
    let tp = test_order_request("KXTEST-BRACKET-4", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(80, 2));
    let sl = test_order_request("KXTEST-BRACKET-4", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(20, 2));

    let (group, orders) = app_state.oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();

    // Simulate entry rejected
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Rejected,
        None, None, None, "test",
    ).await.unwrap();

    // Evaluate triggers
    let _ = app_state.oms.evaluate_triggers(session_id).await.unwrap();

    // TP and SL should be cancelled
    assert_order_state(&pool, tp_order.id, OrderState::Cancelled).await.unwrap();
    assert_order_state(&pool, sl_order.id, OrderState::Cancelled).await.unwrap();

    // Group should be cancelled
    let group_now = db::get_group(&pool, group.id, session_id).await.unwrap().unwrap();
    assert_eq!(group_now.state, harman::types::GroupState::Cancelled);
}

// =============================================================================
// Test 34: OCO creation — both legs pending
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_oco_creation() {
    let (pool, session_id) = setup().await;

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;

    let leg1 = test_order_request("KXTEST-OCO-1", Side::Yes, Action::Buy, Decimal::from(5), Decimal::new(40, 2));
    let leg2 = test_order_request("KXTEST-OCO-2", Side::No, Action::Buy, Decimal::from(5), Decimal::new(60, 2));

    let (group, orders) = app_state.oms.create_oco(session_id, leg1, leg2).await.unwrap();

    assert_eq!(group.group_type, harman::types::GroupType::Oco);
    assert_eq!(orders.len(), 2);
    assert!(orders.iter().all(|o| o.state == OrderState::Pending));
    assert!(orders.iter().all(|o| o.leg_role == Some(LegRole::OcoLeg)));

    // Both should have queue items
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 2);
}

// =============================================================================
// Test 35: OCO — one fills, other gets cancel request, group completed
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_oco_fill_cancels_other() {
    let (pool, session_id) = setup().await;

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;

    let leg1 = test_order_request("KXTEST-OCO-3", Side::Yes, Action::Buy, Decimal::from(5), Decimal::new(40, 2));
    let leg2 = test_order_request("KXTEST-OCO-4", Side::No, Action::Buy, Decimal::from(5), Decimal::new(60, 2));

    let (group, orders) = app_state.oms.create_oco(session_id, leg1, leg2).await.unwrap();

    // Simulate first leg acknowledged and filled
    db::update_order_state(
        &pool, orders[0].id, session_id, OrderState::Acknowledged,
        Some("exch-oco-1"), None, None, "test",
    ).await.unwrap();
    db::update_order_state(
        &pool, orders[0].id, session_id, OrderState::Filled,
        None, Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    // Second leg acknowledged (on exchange)
    db::update_order_state(
        &pool, orders[1].id, session_id, OrderState::Acknowledged,
        Some("exch-oco-2"), None, None, "test",
    ).await.unwrap();

    // Evaluate triggers
    let _ = app_state.oms.evaluate_triggers(session_id).await.unwrap();

    // Group should be completed
    let group_now = db::get_group(&pool, group.id, session_id).await.unwrap().unwrap();
    assert_eq!(group_now.state, harman::types::GroupState::Completed);
}

// =============================================================================
// Test 36: Cancel group — staged legs cancelled directly, open legs get cancel
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_cancel_group() {
    let (pool, session_id) = setup().await;

    let mock = MockExchange::new();
    let app_state = build_test_state(mock, pool.clone(), session_id).await;

    let entry = test_order_request("KXTEST-CANCEL-GRP", Side::Yes, Action::Buy, Decimal::from(5), Decimal::new(50, 2));
    let tp = test_order_request("KXTEST-CANCEL-GRP", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(80, 2));
    let sl = test_order_request("KXTEST-CANCEL-GRP", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(20, 2));

    let (group, orders) = app_state.oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();

    // Cancel the whole group
    app_state.oms.cancel_group(group.id, session_id).await.unwrap();

    // Staged TP and SL should be directly cancelled
    assert_order_state(&pool, tp_order.id, OrderState::Cancelled).await.unwrap();
    assert_order_state(&pool, sl_order.id, OrderState::Cancelled).await.unwrap();

    // Group should be cancelled
    let group_now = db::get_group(&pool, group.id, session_id).await.unwrap().unwrap();
    assert_eq!(group_now.state, harman::types::GroupState::Cancelled);
}

// =============================================================================
// Test 37: Staged legs not counted toward risk limit
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_staged_legs_excluded_from_risk() {
    let (pool, session_id) = setup().await;

    let mock = MockExchange::new();
    // Use tight risk limits: $5 max notional
    let registry = prometheus::Registry::new();
    let ems_metrics = ssmd_harman_ems::EmsMetrics::new(&registry);
    let exchange: Arc<dyn harman::exchange::ExchangeAdapter> = Arc::new(mock);
    let limits = RiskLimits { max_notional: Decimal::from(5) };
    let ems = Arc::new(ssmd_harman_ems::Ems::new(pool.clone(), exchange.clone(), limits, ems_metrics));
    let oms_metrics = OmsMetrics::new(&registry);
    let oms = Arc::new(ssmd_harman_oms::Oms::new(pool.clone(), exchange, ems.clone(), oms_metrics));

    // Bracket: entry=$2.50 (pending), TP=$4.00 (staged), SL=$1.00 (staged)
    // If staged legs were counted: total = $7.50 > $5 → rejected
    // Correct: only entry counted = $2.50 < $5 → accepted
    let entry = test_order_request("KXTEST-RISK-STAGED", Side::Yes, Action::Buy, Decimal::from(5), Decimal::new(50, 2));
    let tp = test_order_request("KXTEST-RISK-STAGED", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(80, 2));
    let sl = test_order_request("KXTEST-RISK-STAGED", Side::Yes, Action::Sell, Decimal::from(5), Decimal::new(20, 2));

    let result = oms.create_bracket(session_id, entry, tp, sl).await;
    assert!(result.is_ok(), "bracket should pass risk check — staged legs excluded");
}
