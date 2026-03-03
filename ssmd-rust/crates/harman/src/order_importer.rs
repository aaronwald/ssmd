use tracing::{debug, error, info, warn};

use deadpool_postgres::Pool;

use crate::db;
use crate::types::{ExchangeOrder, Order};

/// Import external resting orders from the exchange that are not tracked locally.
///
/// External orders are those placed outside of harman (e.g., via the exchange website).
/// They are imported as synthetic orders in 'acknowledged' state.
///
/// Returns the count of newly imported orders.
pub async fn import_external_orders(
    pool: &Pool,
    session_id: i64,
    exchange_orders: &[ExchangeOrder],
    local_orders: &[Order],
    check_db_exists: bool,
    actor: &str,
) -> Result<u64, String> {
    let mut count = 0u64;

    for order in exchange_orders {
        // Check if we already track this order locally (in-memory check)
        let known = local_orders
            .iter()
            .any(|o| o.exchange_order_id.as_deref() == Some(&order.exchange_order_id));

        if known {
            continue;
        }

        // Optional DB-level duplicate check (reconciliation uses this, recovery skips it)
        if check_db_exists {
            match db::order_exists(pool, &order.exchange_order_id).await {
                Ok(true) => {
                    debug!(
                        exchange_order_id = %order.exchange_order_id,
                        "order already exists, skipping external import"
                    );
                    continue;
                }
                Ok(false) => {} // truly external, proceed with import
                Err(e) => {
                    warn!(
                        error = %e,
                        exchange_order_id = %order.exchange_order_id,
                        "order existence check failed, proceeding with import"
                    );
                }
            }
        }

        info!(
            exchange_order_id = %order.exchange_order_id,
            ticker = %order.ticker,
            side = ?order.side,
            action = ?order.action,
            quantity = %order.quantity,
            price = %order.price_dollars,
            client_order_id = ?order.client_order_id,
            actor,
            "importing external resting order"
        );

        match db::create_external_resting_order(
            pool,
            &db::ExternalOrderParams {
                session_id,
                exchange_order_id: &order.exchange_order_id,
                ticker: &order.ticker,
                side: order.side,
                action: order.action,
                quantity: order.quantity,
                price_dollars: order.price_dollars,
            },
        )
        .await
        {
            Ok(_order_id) => {
                count += 1;
            }
            Err(e) => {
                error!(
                    error = %e,
                    exchange_order_id = %order.exchange_order_id,
                    "{}: failed to import external resting order", actor
                );
            }
        }
    }

    Ok(count)
}
