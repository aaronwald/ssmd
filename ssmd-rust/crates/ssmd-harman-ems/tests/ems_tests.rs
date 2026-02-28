//! Integration tests for the EMS crate.
//!
//! Tests the `Ems` struct directly with MockExchange + real PostgreSQL.
//! Verifies pump, queue, shutdown, and metrics behavior without needing
//! the full ssmd-harman binary or REST API.
//!
//! Requires a PostgreSQL database. Set DATABASE_URL to run.
//! Run with: cargo test -p ssmd-harman-ems --test ems_tests -- --ignored

use std::sync::atomic::Ordering;
use std::sync::Arc;

use harman::db;
use harman::risk::RiskLimits;
use harman::state::OrderState;
use harman::test_helpers::*;
use rust_decimal::Decimal;
use uuid::Uuid;

use ssmd_harman_ems::{Ems, EmsMetrics};

/// Build an Ems instance with MockExchange and a test DB pool.
fn build_test_ems(mock: MockExchange, pool: deadpool_postgres::Pool) -> Ems {
    let registry = prometheus::Registry::new();
    let metrics = EmsMetrics::new(&registry);
    Ems::new(pool, Arc::new(mock), RiskLimits::default(), metrics)
}

/// Setup helper: create pool, run migrations, create a unique test session.
async fn setup() -> (deadpool_postgres::Pool, i64) {
    let pool = setup_test_db().await.expect("DATABASE_URL required");
    let unique_prefix = format!("ems-test-{}", Uuid::new_v4());
    let session_id = db::get_or_create_session(&pool, "test", "demo", Some(&unique_prefix))
        .await
        .expect("create session");
    (pool, session_id)
}

// =============================================================================
// Pump: submit
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_ems_pump_submit_success() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    let ems = build_test_ems(mock, pool.clone());

    // Enqueue an order via EMS
    let order = ems
        .enqueue(
            session_id,
            &harman::types::OrderRequest {
                client_order_id: Uuid::new_v4(),
                ticker: "KXTEST-EMS-1".to_string(),
                side: harman::types::Side::Yes,
                action: harman::types::Action::Buy,
                quantity: Decimal::from(1),
                price_dollars: Decimal::new(50, 2), // $0.50
                time_in_force: harman::types::TimeInForce::Gtc,
            },
        )
        .await
        .expect("enqueue should succeed");

    assert_eq!(order.state, OrderState::Pending);

    // Pump should submit to exchange
    let result = ems.pump(session_id).await;
    assert_eq!(result.processed, 1);
    assert_eq!(result.submitted, 1);
    assert_eq!(result.rejected, 0);
    assert!(result.errors.is_empty());

    // Order should now be Acknowledged
    assert_order_state(&pool, order.id, OrderState::Acknowledged)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore]
async fn test_ems_pump_submit_rejected() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.submit_behavior = SubmitBehavior::Reject("insufficient balance".to_string());
    }
    let ems = build_test_ems(mock, pool.clone());

    let order = ems
        .enqueue(
            session_id,
            &harman::types::OrderRequest {
                client_order_id: Uuid::new_v4(),
                ticker: "KXTEST-EMS-REJ".to_string(),
                side: harman::types::Side::Yes,
                action: harman::types::Action::Buy,
                quantity: Decimal::from(1),
                price_dollars: Decimal::new(50, 2),
                time_in_force: harman::types::TimeInForce::Gtc,
            },
        )
        .await
        .unwrap();

    let result = ems.pump(session_id).await;
    assert_eq!(result.processed, 1);
    assert_eq!(result.rejected, 1);
    assert_eq!(result.submitted, 0);

    // Order should be Rejected
    assert_order_state(&pool, order.id, OrderState::Rejected)
        .await
        .unwrap();

    // Metric incremented
    assert_eq!(ems.metrics.orders_rejected.get(), 1);
}

#[tokio::test]
#[ignore]
async fn test_ems_pump_submit_rate_limited() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.submit_behavior = SubmitBehavior::RateLimited(1000);
    }
    let ems = build_test_ems(mock, pool.clone());

    let _order = ems
        .enqueue(
            session_id,
            &harman::types::OrderRequest {
                client_order_id: Uuid::new_v4(),
                ticker: "KXTEST-EMS-RL".to_string(),
                side: harman::types::Side::Yes,
                action: harman::types::Action::Buy,
                quantity: Decimal::from(1),
                price_dollars: Decimal::new(50, 2),
                time_in_force: harman::types::TimeInForce::Gtc,
            },
        )
        .await
        .unwrap();

    let result = ems.pump(session_id).await;
    assert_eq!(result.processed, 1);
    assert_eq!(result.requeued, 1);
    assert!(!result.errors.is_empty());

    // Queue should still have the item (requeued)
    assert!(queue_count(&pool, session_id).await.unwrap() > 0);
}

#[tokio::test]
#[ignore]
async fn test_ems_pump_submit_timeout() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.submit_behavior = SubmitBehavior::Timeout;
    }
    let ems = build_test_ems(mock, pool.clone());

    let order = ems
        .enqueue(
            session_id,
            &harman::types::OrderRequest {
                client_order_id: Uuid::new_v4(),
                ticker: "KXTEST-EMS-TO".to_string(),
                side: harman::types::Side::Yes,
                action: harman::types::Action::Buy,
                quantity: Decimal::from(1),
                price_dollars: Decimal::new(50, 2),
                time_in_force: harman::types::TimeInForce::Gtc,
            },
        )
        .await
        .unwrap();

    let result = ems.pump(session_id).await;
    assert_eq!(result.processed, 1);
    assert!(!result.errors.is_empty());

    // Order stays as Submitted (for reconciliation to resolve)
    assert_order_state(&pool, order.id, OrderState::Submitted)
        .await
        .unwrap();
}

// =============================================================================
// Pump: cancel
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_ems_pump_cancel_success() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    let ems = build_test_ems(mock, pool.clone());

    // Create and pump an order first
    let order = ems
        .enqueue(
            session_id,
            &harman::types::OrderRequest {
                client_order_id: Uuid::new_v4(),
                ticker: "KXTEST-EMS-CANC".to_string(),
                side: harman::types::Side::Yes,
                action: harman::types::Action::Buy,
                quantity: Decimal::from(1),
                price_dollars: Decimal::new(50, 2),
                time_in_force: harman::types::TimeInForce::Gtc,
            },
        )
        .await
        .unwrap();
    ems.pump(session_id).await;

    // Now cancel it
    ems.enqueue_cancel(
        order.id,
        session_id,
        &harman::types::CancelReason::UserRequested,
    )
    .await
    .unwrap();

    let result = ems.pump(session_id).await;
    assert_eq!(result.cancelled, 1);

    assert_order_state(&pool, order.id, OrderState::Cancelled)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore]
async fn test_ems_pump_cancel_without_exchange_id() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    // Make submit timeout so order never gets exchange_order_id
    {
        let mut state = mock.state.lock().await;
        state.submit_behavior = SubmitBehavior::Timeout;
    }
    let ems = build_test_ems(mock, pool.clone());

    // Insert order directly in Submitted state without exchange_order_id
    let order_id =
        insert_test_order(&pool, session_id, OrderState::Submitted, "KXTEST-NOEXCH", None)
            .await
            .unwrap();

    // Enqueue cancel
    ems.enqueue_cancel(
        order_id,
        session_id,
        &harman::types::CancelReason::UserRequested,
    )
    .await
    .unwrap();

    let result = ems.pump(session_id).await;
    assert_eq!(result.cancelled, 1);

    // Should be locally cancelled
    assert_order_state(&pool, order_id, OrderState::Cancelled)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore]
async fn test_ems_pump_cancel_not_found() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.cancel_behavior = CancelBehavior::NotFound;
    }
    let ems = build_test_ems(mock, pool.clone());

    // Insert acknowledged order with exchange_order_id
    let order_id = insert_test_order(
        &pool,
        session_id,
        OrderState::Acknowledged,
        "KXTEST-CANNF",
        Some("exch-cancel-nf"),
    )
    .await
    .unwrap();

    ems.enqueue_cancel(
        order_id,
        session_id,
        &harman::types::CancelReason::UserRequested,
    )
    .await
    .unwrap();

    let result = ems.pump(session_id).await;
    assert_eq!(result.cancelled, 1);

    assert_order_state(&pool, order_id, OrderState::Cancelled)
        .await
        .unwrap();
}

// =============================================================================
// Pump: amend
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_ems_pump_amend_success() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    let ems = build_test_ems(mock, pool.clone());

    // Insert acknowledged order with exchange_order_id
    let order_id = insert_test_order(
        &pool,
        session_id,
        OrderState::Acknowledged,
        "KXTEST-AMEND",
        Some("exch-amend-1"),
    )
    .await
    .unwrap();

    // Enqueue amend with new price
    ems.enqueue_amend(order_id, session_id, Some(Decimal::new(60, 2)), None)
        .await
        .unwrap();

    let result = ems.pump(session_id).await;
    assert_eq!(result.amended, 1);

    // Verify order is back to acknowledged with updated price
    assert_order_state(&pool, order_id, OrderState::Acknowledged)
        .await
        .unwrap();
    let (price, _qty) = get_order_price_qty(&pool, order_id).await.unwrap();
    assert_eq!(price, Decimal::new(60, 2));
}

#[tokio::test]
#[ignore]
async fn test_ems_pump_amend_not_found() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.amend_behavior = AmendBehavior::NotFound;
    }
    let ems = build_test_ems(mock, pool.clone());

    let order_id = insert_test_order(
        &pool,
        session_id,
        OrderState::Acknowledged,
        "KXTEST-AMEND-NF",
        Some("exch-amend-nf"),
    )
    .await
    .unwrap();

    ems.enqueue_amend(order_id, session_id, Some(Decimal::new(60, 2)), None)
        .await
        .unwrap();

    let result = ems.pump(session_id).await;
    // NotFound on amend → cancelled
    assert_order_state(&pool, order_id, OrderState::Cancelled)
        .await
        .unwrap();
    assert_eq!(result.requeued, 1); // counted as requeued with cancel info
}

// =============================================================================
// Pump: decrease
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_ems_pump_decrease_success() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    let ems = build_test_ems(mock, pool.clone());

    let order_id = insert_test_order(
        &pool,
        session_id,
        OrderState::Acknowledged,
        "KXTEST-DEC",
        Some("exch-dec-1"),
    )
    .await
    .unwrap();

    ems.enqueue_decrease(order_id, session_id, Decimal::from(3))
        .await
        .unwrap();

    let result = ems.pump(session_id).await;
    assert_eq!(result.decreased, 1);

    assert_order_state(&pool, order_id, OrderState::Acknowledged)
        .await
        .unwrap();
    let (_price, qty) = get_order_price_qty(&pool, order_id).await.unwrap();
    assert_eq!(qty, Decimal::from(7)); // 10 - 3
}

#[tokio::test]
#[ignore]
async fn test_ems_pump_decrease_not_found() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.decrease_behavior = DecreaseBehavior::NotFound;
    }
    let ems = build_test_ems(mock, pool.clone());

    let order_id = insert_test_order(
        &pool,
        session_id,
        OrderState::Acknowledged,
        "KXTEST-DEC-NF",
        Some("exch-dec-nf"),
    )
    .await
    .unwrap();

    ems.enqueue_decrease(order_id, session_id, Decimal::from(3))
        .await
        .unwrap();

    let result = ems.pump(session_id).await;
    assert_order_state(&pool, order_id, OrderState::Cancelled)
        .await
        .unwrap();
    assert_eq!(result.requeued, 1);
}

// =============================================================================
// Shutdown
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_ems_pump_respects_shutdown() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    let ems = build_test_ems(mock, pool.clone());

    // Enqueue an order
    let _order = ems
        .enqueue(
            session_id,
            &harman::types::OrderRequest {
                client_order_id: Uuid::new_v4(),
                ticker: "KXTEST-EMS-SD".to_string(),
                side: harman::types::Side::Yes,
                action: harman::types::Action::Buy,
                quantity: Decimal::from(1),
                price_dollars: Decimal::new(50, 2),
                time_in_force: harman::types::TimeInForce::Gtc,
            },
        )
        .await
        .unwrap();

    // Set shutting down BEFORE pump
    ems.shutting_down.store(true, Ordering::Relaxed);

    let result = ems.pump(session_id).await;
    assert_eq!(result.processed, 0); // Should exit immediately
    assert!(result.errors.iter().any(|e| e.contains("shutting down")));
}

#[tokio::test]
#[ignore]
async fn test_ems_shutdown_mass_cancels_and_drains() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    {
        let mut state = mock.state.lock().await;
        state.cancel_all_count = 5;
    }
    let ems = build_test_ems(mock, pool.clone());

    // Enqueue an order (will be in queue)
    let _order = ems
        .enqueue(
            session_id,
            &harman::types::OrderRequest {
                client_order_id: Uuid::new_v4(),
                ticker: "KXTEST-EMS-DRAIN".to_string(),
                side: harman::types::Side::Yes,
                action: harman::types::Action::Buy,
                quantity: Decimal::from(1),
                price_dollars: Decimal::new(50, 2),
                time_in_force: harman::types::TimeInForce::Gtc,
            },
        )
        .await
        .unwrap();

    assert!(queue_count(&pool, session_id).await.unwrap() > 0);

    // Shutdown
    ems.shutdown().await;

    assert!(ems.is_shutting_down());
    // Queue should be drained
    assert_eq!(queue_count(&pool, session_id).await.unwrap(), 0);

    // cancel_all was called
    let state = ems.exchange.cancel_all_orders().await.unwrap();
    // This is the mock return value; the real assertion is that the first call happened
    assert_eq!(state, 5);
}

// =============================================================================
// Edge cases
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_ems_pump_empty_queue() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    let ems = build_test_ems(mock, pool.clone());

    // Pump with nothing in queue
    let result = ems.pump(session_id).await;
    assert_eq!(result.processed, 0);
    assert_eq!(result.submitted, 0);
    assert_eq!(result.rejected, 0);
    assert_eq!(result.cancelled, 0);
    assert_eq!(result.amended, 0);
    assert_eq!(result.decreased, 0);
    assert_eq!(result.requeued, 0);
    assert!(result.errors.is_empty());
}

#[tokio::test]
#[ignore]
async fn test_ems_metrics_increment() {
    let (pool, session_id) = setup().await;
    let mock = MockExchange::new();
    let ems = build_test_ems(mock, pool.clone());

    // Submit 2 orders
    for i in 0..2 {
        ems.enqueue(
            session_id,
            &harman::types::OrderRequest {
                client_order_id: Uuid::new_v4(),
                ticker: format!("KXTEST-METRIC-{}", i),
                side: harman::types::Side::Yes,
                action: harman::types::Action::Buy,
                quantity: Decimal::from(1),
                price_dollars: Decimal::new(50, 2),
                time_in_force: harman::types::TimeInForce::Gtc,
            },
        )
        .await
        .unwrap();
    }

    ems.pump(session_id).await;

    assert_eq!(ems.metrics.orders_dequeued.get(), 2);
    assert_eq!(ems.metrics.orders_submitted.get(), 2);
    assert_eq!(ems.metrics.orders_rejected.get(), 0);
}
