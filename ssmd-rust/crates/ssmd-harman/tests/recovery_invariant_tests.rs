//! Integration tests for the recovery invariant check: find_filled_orders_without_fills.
//!
//! Verifies that the fill integrity invariant correctly identifies orders in
//! Filled state that have no corresponding fill records (orphan filled orders).
//!
//! Requires a PostgreSQL database. Set DATABASE_URL to run.
//! Run with: cargo test -p ssmd-harman --test recovery_invariant_tests -- --ignored --test-threads=1

use chrono::Utc;
use harman::db;
use harman::state::OrderState;
use harman::test_helpers::*;
use rust_decimal::Decimal;

/// Setup helper: create pool, run migrations, get/create a test session, and
/// clean all data from it. Must run with --test-threads=1 for isolation.
async fn setup() -> (deadpool_postgres::Pool, i64) {
    let pool = setup_test_db().await.expect("setup_test_db failed");
    let session_id = setup_clean_session(&pool).await.expect("setup_clean_session failed");
    (pool, session_id)
}

// =============================================================================
// Test 1: Filled order WITH fills is NOT flagged as an orphan
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_filled_order_with_fills_not_flagged() {
    let (pool, session_id) = setup().await;

    let order_id = insert_test_order(
        &pool, session_id, OrderState::Filled, "KXTEST-RI-1", Some("exch-ri1"),
    )
    .await
    .unwrap();

    // Record a fill for this order
    db::record_fill(
        &pool,
        order_id,
        session_id,
        "trade-ri1",
        Decimal::new(50, 2),
        Decimal::from(10),
        true,
        Utc::now(),
    )
    .await
    .unwrap();

    let orphans = db::find_filled_orders_without_fills(&pool, session_id)
        .await
        .unwrap();

    assert!(
        orphans.is_empty(),
        "filled order with fills should not be flagged, got: {:?}",
        orphans
    );
}

// =============================================================================
// Test 2: Filled order WITHOUT fills IS flagged as an orphan
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_filled_order_without_fills_is_flagged() {
    let (pool, session_id) = setup().await;

    let order_id = insert_test_order(
        &pool, session_id, OrderState::Filled, "KXTEST-RI-2", Some("exch-ri2"),
    )
    .await
    .unwrap();

    // No fills recorded — this is an invariant violation
    let orphans = db::find_filled_orders_without_fills(&pool, session_id)
        .await
        .unwrap();

    assert_eq!(
        orphans,
        vec![order_id],
        "filled order without fills should be flagged"
    );
}

// =============================================================================
// Test 3: Cancelled order without fills is NOT flagged
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_cancelled_order_without_fills_not_flagged() {
    let (pool, session_id) = setup().await;

    let _order_id = insert_test_order(
        &pool, session_id, OrderState::Cancelled, "KXTEST-RI-3", Some("exch-ri3"),
    )
    .await
    .unwrap();

    // No fills — but order is Cancelled, not Filled
    let orphans = db::find_filled_orders_without_fills(&pool, session_id)
        .await
        .unwrap();

    assert!(
        orphans.is_empty(),
        "cancelled order without fills should not be flagged, got: {:?}",
        orphans
    );
}
