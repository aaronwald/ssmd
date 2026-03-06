use std::collections::{HashMap, HashSet};
use tracing::{debug, error, info};

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
            if order.state.is_terminal() {
                continue;
            }
            let filled_qty = db::get_filled_quantity(pool, *order_id).await?;
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
                }
            }
        }
    }

    Ok(result)
}
