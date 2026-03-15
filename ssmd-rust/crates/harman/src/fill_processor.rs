use std::collections::{HashMap, HashSet};
use tracing::{debug, error, info, warn};

use deadpool_postgres::Pool;
use rust_decimal::Decimal;

use crate::db;
use crate::state::OrderState;
use crate::types::{ExchangeFill, Order};

/// Result of a fill import operation.
#[derive(Debug, Default)]
pub struct FillImportResult {
    /// Number of new fills recorded for known orders.
    pub recorded: u64,
    /// Number of external fills imported (unknown exchange_order_id → synthetic order created).
    pub external_imported: u64,
    /// Number of order state updates (e.g., Acknowledged → Filled).
    pub state_updates: u64,
    /// Order IDs that transitioned to Filled state during this import.
    pub newly_filled_order_ids: Vec<i64>,
}

/// Import fills into the database, handling both known and external fills.
///
/// For each fill:
/// - If the fill's `order_id` matches a known order's `exchange_order_id`, record the fill.
/// - If no matching order exists, create a synthetic external order and record the fill.
///   Fills are sacrosanct — never dropped.
///
/// After recording all fills, updates order states for orders that received new fills
/// (Acknowledged/PartiallyFilled → Filled/PartiallyFilled based on filled quantity).
pub async fn import_fills(
    pool: &Pool,
    session_id: i64,
    fills: &[ExchangeFill],
    session_orders: &[Order],
    actor: &str,
) -> Result<FillImportResult, String> {
    let mut result = FillImportResult::default();

    // Build lookup: exchange_order_id → &Order
    let mut order_by_eid: HashMap<&str, &Order> = HashMap::new();
    for o in session_orders {
        if let Some(eid) = &o.exchange_order_id {
            order_by_eid.insert(eid, o);
        }
    }

    let mut orders_with_new_fills: HashSet<i64> = HashSet::new();

    for fill in fills {
        // Skip fills with zero or negative quantity (exchange data anomaly).
        // These have no economic impact and cannot be stored (fills_quantity_check).
        if fill.quantity <= Decimal::ZERO {
            warn!(
                trade_id = %fill.trade_id,
                exchange_order_id = %fill.order_id,
                quantity = %fill.quantity,
                "skipping fill with non-positive quantity from exchange"
            );
            continue;
        }

        if let Some(order) = order_by_eid.get(fill.order_id.as_str()) {
            let inserted = db::record_fill(
                pool,
                order.id,
                session_id,
                &fill.trade_id,
                fill.price_dollars,
                fill.quantity,
                fill.is_taker,
                fill.filled_at,
            )
            .await?;

            if inserted {
                debug!(
                    order_id = order.id,
                    trade_id = %fill.trade_id,
                    actor,
                    "recorded fill"
                );
                orders_with_new_fills.insert(order.id);
                result.recorded += 1;
            }
        } else {
            // No matching order — external fill. Fills are sacrosanct: never drop.
            info!(
                trade_id = %fill.trade_id,
                exchange_order_id = %fill.order_id,
                ticker = %fill.ticker,
                client_order_id = ?fill.client_order_id,
                actor,
                "importing external fill"
            );
            match db::create_external_order(
                pool,
                &db::ExternalOrderParams {
                    session_id,
                    exchange_order_id: &fill.order_id,
                    ticker: &fill.ticker,
                    side: fill.side,
                    action: fill.action,
                    quantity: fill.quantity,
                    price_dollars: fill.price_dollars,
                },
            )
            .await
            {
                Ok(order_id) => {
                    let inserted = db::record_fill(
                        pool,
                        order_id,
                        session_id,
                        &fill.trade_id,
                        fill.price_dollars,
                        fill.quantity,
                        fill.is_taker,
                        fill.filled_at,
                    )
                    .await?;
                    if inserted {
                        result.recorded += 1;
                        result.external_imported += 1;
                    }
                }
                Err(e) => {
                    error!(
                        error = %e,
                        trade_id = %fill.trade_id,
                        "{}: failed to import external fill", actor
                    );
                }
            }
        }
    }

    // Update order states for orders that received new fills
    for order_id in &orders_with_new_fills {
        if let Some(order) = session_orders.iter().find(|o| o.id == *order_id) {
            let filled_qty = db::get_filled_quantity(pool, *order_id).await?;

            if order.state.is_terminal() {
                // Order already terminal — verify fill count integrity
                if order.state == OrderState::Filled && filled_qty == Decimal::ZERO {
                    error!(
                        order_id = order.id,
                        actor,
                        "INVARIANT VIOLATION: order in Filled state with zero fills"
                    );
                }
                if filled_qty > order.quantity {
                    warn!(
                        order_id = order.id,
                        filled_qty = %filled_qty,
                        order_qty = %order.quantity,
                        actor,
                        "overfill detected — fills exceed order quantity (fill recorded, not dropped)"
                    );
                }
                continue;
            }

            let new_state = if filled_qty >= order.quantity {
                OrderState::Filled
            } else if filled_qty > Decimal::ZERO {
                OrderState::PartiallyFilled
            } else {
                continue;
            };

            if new_state != order.state {
                info!(
                    order_id = order.id,
                    from = %order.state,
                    to = %new_state,
                    filled_qty = %filled_qty,
                    order_qty = %order.quantity,
                    actor,
                    "updated order state from fill"
                );
                if let Err(e) = db::update_order_state(
                    pool,
                    *order_id,
                    session_id,
                    new_state,
                    None,
                    None,
                    actor,
                )
                .await
                {
                    error!(error = %e, order_id, "{}: failed to update order state after fill", actor);
                } else {
                    result.state_updates += 1;
                    if new_state == OrderState::Filled {
                        result.newly_filled_order_ids.push(*order_id);
                    }
                }
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{insert_test_order, mock_fill, setup_clean_session, setup_test_db};
    use crate::state::OrderState;
    use rust_decimal::Decimal;

    static TEST_POOL: tokio::sync::OnceCell<Pool> = tokio::sync::OnceCell::const_new();

    async fn get_pool() -> &'static Pool {
        TEST_POOL
            .get_or_init(|| async { setup_test_db().await.unwrap() })
            .await
    }

    #[tokio::test]
    #[ignore] // requires PostgreSQL (Docker or DATABASE_URL)
    async fn test_fill_transitions_acknowledged_to_filled() {
        let pool = get_pool().await;
        let session_id = setup_clean_session(pool).await.unwrap();

        let order_id = insert_test_order(
            pool, session_id, OrderState::Acknowledged, "TICKER-A", Some("exch-1"),
        ).await.unwrap();

        let fill = mock_fill("exch-1", "TICKER-A", Decimal::from(10), Decimal::new(50, 2));
        let orders = db::list_orders(pool, session_id, None).await.unwrap();

        let result = import_fills(pool, session_id, &[fill], &orders, "test").await.unwrap();

        assert_eq!(result.recorded, 1);
        assert_eq!(result.state_updates, 1);
        assert!(result.newly_filled_order_ids.contains(&order_id));

        let filled_qty = db::get_filled_quantity(pool, order_id).await.unwrap();
        assert_eq!(filled_qty, Decimal::from(10));
    }

    #[tokio::test]
    #[ignore]
    async fn test_partial_fill_transitions_to_partially_filled() {
        let pool = get_pool().await;
        let session_id = setup_clean_session(pool).await.unwrap();

        let order_id = insert_test_order(
            pool, session_id, OrderState::Acknowledged, "TICKER-B", Some("exch-2"),
        ).await.unwrap();

        let fill = mock_fill("exch-2", "TICKER-B", Decimal::from(5), Decimal::new(50, 2));
        let orders = db::list_orders(pool, session_id, None).await.unwrap();

        let result = import_fills(pool, session_id, &[fill], &orders, "test").await.unwrap();

        assert_eq!(result.recorded, 1);
        assert_eq!(result.state_updates, 1);
        assert!(result.newly_filled_order_ids.is_empty());

        let filled_qty = db::get_filled_quantity(pool, order_id).await.unwrap();
        assert_eq!(filled_qty, Decimal::from(5));
    }

    #[tokio::test]
    #[ignore]
    async fn test_fill_for_terminal_order_still_records() {
        let pool = get_pool().await;
        let session_id = setup_clean_session(pool).await.unwrap();

        let order_id = insert_test_order(
            pool, session_id, OrderState::Filled, "TICKER-C", Some("exch-3"),
        ).await.unwrap();

        let fill = mock_fill("exch-3", "TICKER-C", Decimal::from(10), Decimal::new(50, 2));
        let orders = db::list_orders(pool, session_id, None).await.unwrap();

        let result = import_fills(pool, session_id, &[fill], &orders, "test").await.unwrap();

        assert_eq!(result.recorded, 1);
        assert_eq!(result.state_updates, 0);
        assert!(result.newly_filled_order_ids.is_empty());

        let filled_qty = db::get_filled_quantity(pool, order_id).await.unwrap();
        assert_eq!(filled_qty, Decimal::from(10));
    }

    #[tokio::test]
    #[ignore]
    async fn test_overfill_records_fill_not_dropped() {
        let pool = get_pool().await;
        let session_id = setup_clean_session(pool).await.unwrap();

        let order_id = insert_test_order(
            pool, session_id, OrderState::Acknowledged, "TICKER-D", Some("exch-4"),
        ).await.unwrap();

        let fill1 = mock_fill("exch-4", "TICKER-D", Decimal::from(10), Decimal::new(50, 2));
        let fill2 = mock_fill("exch-4", "TICKER-D", Decimal::from(5), Decimal::new(50, 2));
        let orders = db::list_orders(pool, session_id, None).await.unwrap();

        let result = import_fills(pool, session_id, &[fill1, fill2], &orders, "test").await.unwrap();

        assert_eq!(result.recorded, 2);
        assert!(result.newly_filled_order_ids.contains(&order_id));

        let filled_qty = db::get_filled_quantity(pool, order_id).await.unwrap();
        assert_eq!(filled_qty, Decimal::from(15));
    }

    #[tokio::test]
    #[ignore]
    async fn test_zero_quantity_fill_skipped() {
        let pool = get_pool().await;
        let session_id = setup_clean_session(pool).await.unwrap();

        let _order_id = insert_test_order(
            pool, session_id, OrderState::Acknowledged, "TICKER-E", Some("exch-5"),
        ).await.unwrap();

        let fill = mock_fill("exch-5", "TICKER-E", Decimal::ZERO, Decimal::new(50, 2));
        let orders = db::list_orders(pool, session_id, None).await.unwrap();

        let result = import_fills(pool, session_id, &[fill], &orders, "test").await.unwrap();

        assert_eq!(result.recorded, 0);
    }

    #[tokio::test]
    #[ignore]
    async fn test_duplicate_fill_deduped_by_trade_id() {
        let pool = get_pool().await;
        let session_id = setup_clean_session(pool).await.unwrap();

        let _order_id = insert_test_order(
            pool, session_id, OrderState::Acknowledged, "TICKER-F", Some("exch-6"),
        ).await.unwrap();

        let fill = mock_fill("exch-6", "TICKER-F", Decimal::from(10), Decimal::new(50, 2));
        let orders = db::list_orders(pool, session_id, None).await.unwrap();

        let result1 = import_fills(pool, session_id, &[fill.clone()], &orders, "test").await.unwrap();
        assert_eq!(result1.recorded, 1);

        let result2 = import_fills(pool, session_id, &[fill], &orders, "test").await.unwrap();
        assert_eq!(result2.recorded, 0);
    }

    #[tokio::test]
    #[ignore]
    async fn test_external_fill_creates_synthetic_order() {
        let pool = get_pool().await;
        let session_id = setup_clean_session(pool).await.unwrap();

        // No orders inserted — fill has unknown exchange_order_id
        let fill = mock_fill("unknown-exch-99", "TICKER-G", Decimal::from(5), Decimal::new(50, 2));
        let orders: Vec<Order> = vec![];

        let result = import_fills(pool, session_id, &[fill], &orders, "test").await.unwrap();

        assert_eq!(result.recorded, 1);
        assert_eq!(result.external_imported, 1);
    }
}
