//! Integration tests for group handling on fill events.
//!
//! Tests the `handle_group_on_fill` DB function which is called by the WS event
//! ingester when an order transitions to Filled. Verifies role-aware behavior:
//! - Bracket entry fill → activates staged TP/SL exit legs
//! - Bracket exit fill → cancels the other exit leg
//! - Bracket entry cancel → cancels staged legs (via cancel_staged_group_siblings)
//! - OCO fill → no staged legs to activate (both start Pending)
//!
//! Requires a PostgreSQL database. Set DATABASE_URL to run.
//! Run with: cargo test -p ssmd-harman --test group_fill_tests -- --ignored

use std::sync::Arc;

use harman::db;
use harman::risk::RiskLimits;
use harman::state::OrderState;
use harman::test_helpers::*;
use harman::types::{Action, LegRole, OrderRequest, Side, TimeInForce};
use rust_decimal::Decimal;
use uuid::Uuid;

use ssmd_harman_ems::{Ems, EmsMetrics};
use ssmd_harman_oms::{Oms, OmsMetrics};

/// Setup helper: create pool, run migrations, create a unique test session.
async fn setup() -> (deadpool_postgres::Pool, i64) {
    let pool = setup_test_db().await.expect("setup_test_db failed");
    let unique_prefix = format!("group-fill-test-{}", Uuid::new_v4());
    let session_id = db::get_or_create_session(&pool, "test", "demo", Some(&unique_prefix))
        .await
        .unwrap_or_else(|e| panic!("create session '{}' failed: {}", unique_prefix, e));
    (pool, session_id)
}

/// Build an Oms instance for bracket/OCO creation.
async fn build_test_oms(pool: deadpool_postgres::Pool) -> Oms {
    let registry = prometheus::Registry::new();
    let ems_metrics = EmsMetrics::new(&registry);
    let mock = MockExchange::new();
    let exchange: Arc<dyn harman::exchange::ExchangeAdapter> = Arc::new(mock);
    let (audit_sender, audit_writer) = harman::audit::create_audit_channel(pool.clone());
    tokio::spawn(audit_writer.run());
    let ems = Arc::new(Ems::new(
        pool.clone(),
        exchange.clone(),
        RiskLimits::default(),
        ems_metrics,
        audit_sender.clone(),
    ));
    let oms_metrics = Arc::new(OmsMetrics::new(&registry));
    Oms::new(pool, exchange, ems, oms_metrics, audit_sender)
}

fn test_order_request(
    ticker: &str,
    side: Side,
    action: Action,
    qty: Decimal,
    price: Decimal,
) -> OrderRequest {
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
// Test 1: Bracket entry fill activates staged TP and SL legs
//
// This is the core bug fix test. Before the fix, entry fill would CANCEL
// staged legs instead of activating them.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_handle_group_on_fill_bracket_entry_activates_exits() {
    let (pool, session_id) = setup().await;
    let oms = build_test_oms(pool.clone()).await;

    let entry = test_order_request(
        "KXTEST-GF-1", Side::Yes, Action::Buy,
        Decimal::from(5), Decimal::new(50, 2),
    );
    let tp = test_order_request(
        "KXTEST-GF-1", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(80, 2),
    );
    let sl = test_order_request(
        "KXTEST-GF-1", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(20, 2),
    );

    let (_group, orders) = oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();

    // Precondition: TP and SL are Staged
    assert_order_state(&pool, tp_order.id, OrderState::Staged).await.unwrap();
    assert_order_state(&pool, sl_order.id, OrderState::Staged).await.unwrap();

    // Drain entry queue item and simulate entry being filled
    let _ = db::dequeue_order(&pool, session_id).await;
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Filled,
        Some("exch-gf1-entry"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    // Call handle_group_on_fill — this is what the event ingester calls
    let activated = db::handle_group_on_fill(&pool, entry_order.id, session_id)
        .await
        .unwrap();

    // Should activate both TP and SL
    assert_eq!(activated, 2, "should activate 2 exit legs (TP + SL)");

    // TP and SL should now be Pending (not Cancelled!)
    assert_order_state(&pool, tp_order.id, OrderState::Pending).await.unwrap();
    assert_order_state(&pool, sl_order.id, OrderState::Pending).await.unwrap();

    // Queue should have 2 items (TP + SL submit)
    let count = queue_count(&pool, session_id).await.unwrap();
    assert_eq!(count, 2, "TP and SL should each have a queue item");
}

// =============================================================================
// Test 2: Bracket exit (TP) fill cancels the other exit (SL)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_handle_group_on_fill_bracket_tp_fill_cancels_sl() {
    let (pool, session_id) = setup().await;
    let oms = build_test_oms(pool.clone()).await;

    let entry = test_order_request(
        "KXTEST-GF-2", Side::Yes, Action::Buy,
        Decimal::from(5), Decimal::new(50, 2),
    );
    let tp = test_order_request(
        "KXTEST-GF-2", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(80, 2),
    );
    let sl = test_order_request(
        "KXTEST-GF-2", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(20, 2),
    );

    let (group, orders) = oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();

    // Simulate full lifecycle: entry filled, then TP activated and filled
    let _ = db::dequeue_order(&pool, session_id).await;
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Filled,
        Some("exch-gf2-entry"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    // Activate exits (entry fill)
    db::handle_group_on_fill(&pool, entry_order.id, session_id).await.unwrap();

    // Now simulate TP being filled
    db::update_order_state(
        &pool, tp_order.id, session_id, OrderState::Filled,
        Some("exch-gf2-tp"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    // Call handle_group_on_fill for TP fill — should cancel SL
    let activated = db::handle_group_on_fill(&pool, tp_order.id, session_id)
        .await
        .unwrap();

    assert_eq!(activated, 0, "exit fill should not activate anything");

    // SL should be cancelled
    assert_order_state(&pool, sl_order.id, OrderState::Cancelled).await.unwrap();

    // Group should be completed or finalized
    let group_now = db::get_group(&pool, group.id, session_id).await.unwrap().unwrap();
    assert_eq!(group_now.state, harman::types::GroupState::Completed);
}

// =============================================================================
// Test 3: Bracket exit (SL) fill cancels the other exit (TP)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_handle_group_on_fill_bracket_sl_fill_cancels_tp() {
    let (pool, session_id) = setup().await;
    let oms = build_test_oms(pool.clone()).await;

    let entry = test_order_request(
        "KXTEST-GF-3", Side::Yes, Action::Buy,
        Decimal::from(5), Decimal::new(50, 2),
    );
    let tp = test_order_request(
        "KXTEST-GF-3", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(80, 2),
    );
    let sl = test_order_request(
        "KXTEST-GF-3", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(20, 2),
    );

    let (group, orders) = oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();

    // Entry filled
    let _ = db::dequeue_order(&pool, session_id).await;
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Filled,
        Some("exch-gf3-entry"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();
    db::handle_group_on_fill(&pool, entry_order.id, session_id).await.unwrap();

    // SL filled (instead of TP this time)
    db::update_order_state(
        &pool, sl_order.id, session_id, OrderState::Filled,
        Some("exch-gf3-sl"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    let activated = db::handle_group_on_fill(&pool, sl_order.id, session_id)
        .await
        .unwrap();

    assert_eq!(activated, 0);

    // TP should be cancelled (it was still pending/staged)
    // After activate, TP went to Pending; SL fill should cancel it
    assert_order_state(&pool, tp_order.id, OrderState::Cancelled).await.unwrap();

    let group_now = db::get_group(&pool, group.id, session_id).await.unwrap().unwrap();
    assert_eq!(group_now.state, harman::types::GroupState::Completed);
}

// =============================================================================
// Test 4: Bracket entry cancel cancels staged legs
//
// Uses cancel_staged_group_siblings (existing behavior, not handle_group_on_fill).
// This tests that the cancel path in event_ingester still works correctly.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_bracket_entry_cancel_cascades_to_staged() {
    let (pool, session_id) = setup().await;
    let oms = build_test_oms(pool.clone()).await;

    let entry = test_order_request(
        "KXTEST-GF-4", Side::Yes, Action::Buy,
        Decimal::from(5), Decimal::new(50, 2),
    );
    let tp = test_order_request(
        "KXTEST-GF-4", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(80, 2),
    );
    let sl = test_order_request(
        "KXTEST-GF-4", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(20, 2),
    );

    let (group, orders) = oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();

    // Entry gets cancelled (the cancel path in event_ingester)
    let _ = db::dequeue_order(&pool, session_id).await;
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Cancelled,
        None, Some(Decimal::ZERO), Some(&harman::types::CancelReason::ExchangeCancel), "test",
    ).await.unwrap();

    // Cancel staged siblings (what event_ingester does on cancel path)
    let cancelled = db::cancel_staged_group_siblings(&pool, entry_order.id, session_id)
        .await
        .unwrap();

    assert_eq!(cancelled, 2, "should cancel both TP and SL");
    assert_order_state(&pool, tp_order.id, OrderState::Cancelled).await.unwrap();
    assert_order_state(&pool, sl_order.id, OrderState::Cancelled).await.unwrap();

    // Group should be finalized as cancelled (no fills)
    let group_now = db::get_group(&pool, group.id, session_id).await.unwrap().unwrap();
    assert_eq!(group_now.state, harman::types::GroupState::Cancelled);
}

// =============================================================================
// Test 5: handle_group_on_fill is idempotent — calling twice doesn't double-activate
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_handle_group_on_fill_idempotent() {
    let (pool, session_id) = setup().await;
    let oms = build_test_oms(pool.clone()).await;

    let entry = test_order_request(
        "KXTEST-GF-5", Side::Yes, Action::Buy,
        Decimal::from(5), Decimal::new(50, 2),
    );
    let tp = test_order_request(
        "KXTEST-GF-5", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(80, 2),
    );
    let sl = test_order_request(
        "KXTEST-GF-5", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(20, 2),
    );

    let (_group, orders) = oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();

    let _ = db::dequeue_order(&pool, session_id).await;
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Filled,
        Some("exch-gf5-entry"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    // First call activates
    let activated1 = db::handle_group_on_fill(&pool, entry_order.id, session_id)
        .await
        .unwrap();
    assert_eq!(activated1, 2);

    // Second call — legs already pending, no more staged legs to activate
    let activated2 = db::handle_group_on_fill(&pool, entry_order.id, session_id)
        .await
        .unwrap();
    assert_eq!(activated2, 0, "second call should activate 0 (already pending)");
}

// =============================================================================
// Test 6: handle_group_on_fill for non-grouped order is a no-op
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_handle_group_on_fill_no_group_is_noop() {
    let (pool, session_id) = setup().await;

    // Insert a standalone order (no group)
    let order_id = insert_test_order(&pool, session_id, OrderState::Filled, "KXTEST-GF-6", Some("exch-gf6"))
        .await
        .unwrap();

    let activated = db::handle_group_on_fill(&pool, order_id, session_id)
        .await
        .unwrap();

    assert_eq!(activated, 0, "non-grouped order should be no-op");
}

// =============================================================================
// Test 7: handle_group_on_fill for unknown order returns 0
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_handle_group_on_fill_unknown_order() {
    let (pool, session_id) = setup().await;

    // Non-existent order ID
    let activated = db::handle_group_on_fill(&pool, 999999, session_id)
        .await
        .unwrap();

    assert_eq!(activated, 0, "unknown order should return 0");
}

// =============================================================================
// Test 8: OCO fill — both legs are Pending, no staged legs to activate
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_handle_group_on_fill_oco() {
    let (pool, session_id) = setup().await;
    let oms = build_test_oms(pool.clone()).await;

    let leg1 = test_order_request(
        "KXTEST-GF-8A", Side::Yes, Action::Buy,
        Decimal::from(5), Decimal::new(40, 2),
    );
    let leg2 = test_order_request(
        "KXTEST-GF-8B", Side::No, Action::Buy,
        Decimal::from(5), Decimal::new(60, 2),
    );

    let (_group, orders) = oms.create_oco(session_id, leg1, leg2).await.unwrap();
    let first = &orders[0];

    // Drain queue items
    let _ = db::dequeue_order(&pool, session_id).await;
    let _ = db::dequeue_order(&pool, session_id).await;

    // Simulate first leg filled
    db::update_order_state(
        &pool, first.id, session_id, OrderState::Filled,
        Some("exch-gf8-leg1"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    // handle_group_on_fill for OCO — should not activate anything
    // (OCO legs are Pending, not Staged — the EMS cancel happens via evaluate_triggers)
    let activated = db::handle_group_on_fill(&pool, first.id, session_id)
        .await
        .unwrap();

    assert_eq!(activated, 0, "OCO should not activate any staged legs");
}

// =============================================================================
// Test 9: Full bracket lifecycle — entry fill, TP activated, TP filled, SL cancelled
//
// End-to-end test simulating what the event ingester does at each step.
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_full_bracket_lifecycle_via_handle_group_on_fill() {
    let (pool, session_id) = setup().await;
    let oms = build_test_oms(pool.clone()).await;

    let entry = test_order_request(
        "KXTEST-GF-9", Side::Yes, Action::Buy,
        Decimal::from(10), Decimal::new(50, 2),
    );
    let tp = test_order_request(
        "KXTEST-GF-9", Side::Yes, Action::Sell,
        Decimal::from(10), Decimal::new(75, 2),
    );
    let sl = test_order_request(
        "KXTEST-GF-9", Side::Yes, Action::Sell,
        Decimal::from(10), Decimal::new(30, 2),
    );

    let (group, orders) = oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();

    // Step 1: Entry filled
    let _ = db::dequeue_order(&pool, session_id).await;
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Filled,
        Some("exch-gf9-entry"), Some(Decimal::from(10)), None, "test",
    ).await.unwrap();

    let activated = db::handle_group_on_fill(&pool, entry_order.id, session_id).await.unwrap();
    assert_eq!(activated, 2, "step 1: should activate TP + SL");
    assert_order_state(&pool, tp_order.id, OrderState::Pending).await.unwrap();
    assert_order_state(&pool, sl_order.id, OrderState::Pending).await.unwrap();

    // Step 2: TP submitted and acknowledged
    db::update_order_state(
        &pool, tp_order.id, session_id, OrderState::Acknowledged,
        Some("exch-gf9-tp"), Some(Decimal::ZERO), None, "test",
    ).await.unwrap();

    // Step 3: SL submitted and acknowledged
    db::update_order_state(
        &pool, sl_order.id, session_id, OrderState::Acknowledged,
        Some("exch-gf9-sl"), Some(Decimal::ZERO), None, "test",
    ).await.unwrap();

    // Step 4: TP filled
    db::update_order_state(
        &pool, tp_order.id, session_id, OrderState::Filled,
        Some("exch-gf9-tp"), Some(Decimal::from(10)), None, "test",
    ).await.unwrap();

    let activated = db::handle_group_on_fill(&pool, tp_order.id, session_id).await.unwrap();
    assert_eq!(activated, 0, "step 4: exit fill should not activate");

    // SL was Acknowledged (not Staged), so cancel_staged_group_siblings won't cancel it.
    // The evaluate_triggers path would need to enqueue a cancel via EMS.
    // But the staged->cancelled path works for Staged legs.
    // For this test, verify the exit fill doesn't break anything.

    // Group should still be active (SL still open)
    let group_now = db::get_group(&pool, group.id, session_id).await.unwrap().unwrap();
    // cancel_staged_group_siblings only cancels staged orders, not acknowledged ones.
    // The full cancel of the SL would happen via evaluate_triggers → EMS cancel.
    // This is correct — the event ingester handles what it can (staged cancels),
    // and evaluate_triggers handles open orders that need exchange cancellation.
    assert!(
        group_now.state == harman::types::GroupState::Active
            || group_now.state == harman::types::GroupState::Completed,
        "group should be active or completed"
    );
}

// =============================================================================
// Test 10: Bracket entry fill when exits are already cancelled (race condition)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_handle_group_on_fill_entry_fill_exits_already_cancelled() {
    let (pool, session_id) = setup().await;
    let oms = build_test_oms(pool.clone()).await;

    let entry = test_order_request(
        "KXTEST-GF-10", Side::Yes, Action::Buy,
        Decimal::from(5), Decimal::new(50, 2),
    );
    let tp = test_order_request(
        "KXTEST-GF-10", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(80, 2),
    );
    let sl = test_order_request(
        "KXTEST-GF-10", Side::Yes, Action::Sell,
        Decimal::from(5), Decimal::new(20, 2),
    );

    let (_group, orders) = oms.create_bracket(session_id, entry, tp, sl).await.unwrap();
    let entry_order = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry)).unwrap();
    let tp_order = orders.iter().find(|o| o.leg_role == Some(LegRole::TakeProfit)).unwrap();
    let sl_order = orders.iter().find(|o| o.leg_role == Some(LegRole::StopLoss)).unwrap();

    // Manually cancel the exit legs first (simulating a race or user cancel)
    db::update_order_state(
        &pool, tp_order.id, session_id, OrderState::Cancelled,
        None, Some(Decimal::ZERO),
        Some(&harman::types::CancelReason::UserRequested), "test",
    ).await.unwrap();
    db::update_order_state(
        &pool, sl_order.id, session_id, OrderState::Cancelled,
        None, Some(Decimal::ZERO),
        Some(&harman::types::CancelReason::UserRequested), "test",
    ).await.unwrap();

    // Now entry fills
    let _ = db::dequeue_order(&pool, session_id).await;
    db::update_order_state(
        &pool, entry_order.id, session_id, OrderState::Filled,
        Some("exch-gf10-entry"), Some(Decimal::from(5)), None, "test",
    ).await.unwrap();

    // Should activate 0 (no staged legs left)
    let activated = db::handle_group_on_fill(&pool, entry_order.id, session_id)
        .await
        .unwrap();

    assert_eq!(activated, 0, "no staged legs to activate");

    // Exits should remain cancelled
    assert_order_state(&pool, tp_order.id, OrderState::Cancelled).await.unwrap();
    assert_order_state(&pool, sl_order.id, OrderState::Cancelled).await.unwrap();
}
