//! End-to-end fill integrity tests for the EventIngester.
//!
//! These tests verify that fills are the authority for order state transitions,
//! not OrderUpdate events. The EventIngester deliberately ignores OrderUpdate(Filled)
//! and relies on the fill_processor to transition order state.
//!
//! Requires a PostgreSQL database. Run with:
//! cargo test -p ssmd-harman --features testcontainers -- fill_integrity --test-threads=1

use std::sync::Arc;

use chrono::Utc;
use rust_decimal::Decimal;
use tokio::sync::broadcast;
use uuid::Uuid;

use harman::audit::AuditSender;
use harman::db;
use harman::exchange::ExchangeEvent;
use harman::state::OrderState;
use harman::test_helpers::*;
use harman::types::{Action, Side};

use ssmd_harman_oms::OmsMetrics;
use ssmd_harman_oms::event_ingester::EventIngester;
use ssmd_harman_oms::runner::PumpTrigger;

/// Setup helper: create pool, run migrations, get/create a test session, and
/// clean all data from it. Must run with --test-threads=1 for isolation.
async fn setup() -> (deadpool_postgres::Pool, i64) {
    let pool = setup_test_db().await.expect("setup_test_db failed");
    let session_id = setup_clean_session(&pool).await.expect("setup_clean_session failed");
    (pool, session_id)
}

/// Build an EventIngester for testing.
///
/// Uses a mock exchange, a noop audit sender (channel exists but writer is not
/// spawned — events are buffered and dropped), noop metrics, and a noop pump trigger.
fn build_test_ingester(
    pool: deadpool_postgres::Pool,
    session_id: i64,
) -> EventIngester {
    let registry = prometheus::Registry::new();
    let metrics = Arc::new(OmsMetrics::new(&registry));

    // Create an audit sender with a large buffer. We don't spawn the writer —
    // events are buffered and silently dropped when the channel is full or on shutdown.
    let (tx, _rx) = tokio::sync::mpsc::channel(4096);
    let audit = AuditSender::new(tx);

    let pump_trigger = PumpTrigger::new();

    EventIngester::new(
        pool,
        metrics,
        audit,
        pump_trigger,
        None, // no PriceMonitor
        session_id,
    )
}

fn make_fill_event(exchange_order_id: &str, ticker: &str, qty: i64) -> ExchangeEvent {
    ExchangeEvent::Fill {
        trade_id: format!("trade-{}", Uuid::new_v4()),
        exchange_order_id: exchange_order_id.to_string(),
        ticker: ticker.to_string(),
        side: Side::Yes,
        action: Action::Buy,
        price_dollars: Decimal::new(50, 2),
        quantity: Decimal::from(qty),
        is_taker: true,
        filled_at: Utc::now(),
        client_order_id: None,
    }
}

fn make_order_update_filled(exchange_order_id: &str, ticker: &str, filled_qty: i64) -> ExchangeEvent {
    ExchangeEvent::OrderUpdate {
        exchange_order_id: exchange_order_id.to_string(),
        client_order_id: None,
        ticker: ticker.to_string(),
        status: OrderState::Filled,
        filled_quantity: Decimal::from(filled_qty),
        remaining_quantity: Decimal::ZERO,
        close_cancel_count: None,
    }
}

// =============================================================================
// Test 1: OrderUpdate(Filled) followed by Fill — fill drives state transition
//
// Verifies that OrderUpdate(Filled) is informational only and does NOT
// transition the order state. The Fill event (via fill_processor) is the
// authority for Filled transitions.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_order_update_filled_does_not_transition_state() {
    let (pool, session_id) = setup().await;

    // Insert an Acknowledged order
    let order_id = insert_test_order(
        &pool, session_id, OrderState::Acknowledged, "KXFI-1", Some("exch-fi-1"),
    )
    .await
    .unwrap();

    let (tx, rx) = broadcast::channel(16);
    let ingester = build_test_ingester(pool.clone(), session_id);

    // Send OrderUpdate(Filled) then Fill
    tx.send(make_order_update_filled("exch-fi-1", "KXFI-1", 10)).unwrap();
    tx.send(make_fill_event("exch-fi-1", "KXFI-1", 10)).unwrap();
    drop(tx);

    let result = ingester.run(rx).await;

    assert_eq!(result.fills_recorded, 1, "should record exactly 1 fill");
    assert_eq!(result.events_processed, 2, "should process 2 events");

    // Order should be Filled (fill_processor transitioned it)
    assert_order_state(&pool, order_id, OrderState::Filled).await.unwrap();

    // Verify filled quantity is correct
    let filled_qty = db::get_filled_quantity(&pool, order_id).await.unwrap();
    assert_eq!(filled_qty, Decimal::from(10));
}

// =============================================================================
// Test 2: Fill before OrderUpdate — fill drives state regardless of order
//
// Even when the Fill arrives before the OrderUpdate, the fill_processor
// correctly transitions the order to Filled.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_fill_before_order_update_works() {
    let (pool, session_id) = setup().await;

    let order_id = insert_test_order(
        &pool, session_id, OrderState::Acknowledged, "KXFI-2", Some("exch-fi-2"),
    )
    .await
    .unwrap();

    let (tx, rx) = broadcast::channel(16);
    let ingester = build_test_ingester(pool.clone(), session_id);

    // Fill arrives first, then OrderUpdate
    tx.send(make_fill_event("exch-fi-2", "KXFI-2", 10)).unwrap();
    tx.send(make_order_update_filled("exch-fi-2", "KXFI-2", 10)).unwrap();
    drop(tx);

    let result = ingester.run(rx).await;

    assert_eq!(result.fills_recorded, 1);
    assert_eq!(result.events_processed, 2);

    // Order should be Filled
    assert_order_state(&pool, order_id, OrderState::Filled).await.unwrap();

    let filled_qty = db::get_filled_quantity(&pool, order_id).await.unwrap();
    assert_eq!(filled_qty, Decimal::from(10));
}

// =============================================================================
// Test 3: OrderUpdate(Filled) alone does NOT change order state
//
// This is the core fill integrity invariant: without a Fill event,
// OrderUpdate(Filled) is purely informational and must not transition state.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_order_update_filled_alone_does_not_change_state() {
    let (pool, session_id) = setup().await;

    let order_id = insert_test_order(
        &pool, session_id, OrderState::Acknowledged, "KXFI-3", Some("exch-fi-3"),
    )
    .await
    .unwrap();

    let (tx, rx) = broadcast::channel(16);
    let ingester = build_test_ingester(pool.clone(), session_id);

    // Send ONLY OrderUpdate(Filled) — no Fill event
    tx.send(make_order_update_filled("exch-fi-3", "KXFI-3", 10)).unwrap();
    drop(tx);

    let result = ingester.run(rx).await;

    assert_eq!(result.fills_recorded, 0, "no fills should be recorded");
    assert_eq!(result.events_processed, 1);

    // Order must still be Acknowledged — OrderUpdate(Filled) is informational only
    assert_order_state(&pool, order_id, OrderState::Acknowledged).await.unwrap();

    // No fills recorded
    let filled_qty = db::get_filled_quantity(&pool, order_id).await.unwrap();
    assert_eq!(filled_qty, Decimal::ZERO);
}

// =============================================================================
// Test 4: External fill creates a synthetic order
//
// Fill for an unknown exchange_order_id should create a synthetic order and
// record the fill. Fills are sacrosanct — never dropped.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_external_fill_creates_synthetic_order() {
    let (pool, session_id) = setup().await;

    // No orders exist in the session
    let (tx, rx) = broadcast::channel(16);
    let ingester = build_test_ingester(pool.clone(), session_id);

    // Fill for an unknown exchange_order_id
    tx.send(make_fill_event("unknown-exch-ext-1", "KXFI-4", 5)).unwrap();
    drop(tx);

    let result = ingester.run(rx).await;

    assert_eq!(result.fills_recorded, 1, "external fill must be recorded");
    assert_eq!(result.events_processed, 1);

    // Verify a synthetic order was created by looking up orders in the session
    let orders = db::list_orders(&pool, session_id, None).await.unwrap();
    assert_eq!(orders.len(), 1, "synthetic order should exist");
    assert_eq!(orders[0].exchange_order_id.as_deref(), Some("unknown-exch-ext-1"));
    assert_eq!(orders[0].ticker, "KXFI-4");

    // Fill should be recorded against the synthetic order
    let filled_qty = db::get_filled_quantity(&pool, orders[0].id).await.unwrap();
    assert_eq!(filled_qty, Decimal::from(5));
}
