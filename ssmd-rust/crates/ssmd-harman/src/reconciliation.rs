use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use harman::db;
use harman::state::{self, OrderState};
use harman::types::ExchangeOrderState;

use crate::AppState;

const RECONCILIATION_INTERVAL: Duration = Duration::from_secs(60);
const STALE_THRESHOLD: Duration = Duration::from_secs(30);

/// Run the reconciliation loop.
///
/// Periodically:
/// 1. Discovers missing fills from exchange
/// 2. Resolves stale submitted/pending_cancel orders
pub async fn run(state: Arc<AppState>) {
    info!("reconciliation poller started");

    loop {
        if state.shutting_down.load(Ordering::Relaxed) {
            info!("reconciliation shutting down");
            break;
        }

        tokio::time::sleep(RECONCILIATION_INTERVAL).await;

        if state.shutting_down.load(Ordering::Relaxed) {
            break;
        }

        // 1. Discover fills
        if let Err(e) = discover_fills(&state).await {
            error!(error = %e, "fill discovery failed");
        }

        // 2. Resolve stale orders
        if let Err(e) = resolve_stale_orders(&state).await {
            error!(error = %e, "stale order resolution failed");
        }
    }
}

/// Fetch recent fills from exchange and record any missing ones.
///
/// Loads all orders once and builds a lookup to avoid N+1 queries.
async fn discover_fills(state: &Arc<AppState>) -> Result<(), String> {
    let fills = state
        .exchange
        .get_fills()
        .await
        .map_err(|e| format!("get fills: {}", e))?;

    debug!(count = fills.len(), "fetched exchange fills");

    // Load orders once for the entire fill batch (fixes N+1 query)
    let orders = db::list_orders(&state.pool, None).await?;

    for fill in &fills {
        if let Some(order) = orders
            .iter()
            .find(|o| o.exchange_order_id.as_deref() == Some(&fill.order_id))
        {
            let inserted = db::record_fill(
                &state.pool,
                order.id,
                &fill.trade_id,
                fill.price_cents,
                fill.quantity,
                fill.is_taker,
                fill.filled_at,
            )
            .await?;

            if inserted {
                info!(
                    order_id = order.id,
                    trade_id = %fill.trade_id,
                    "recorded missing fill"
                );
                state.metrics.fills_recorded.inc();
            }
        }
    }

    Ok(())
}

/// Find and resolve orders stuck in ambiguous states
async fn resolve_stale_orders(state: &Arc<AppState>) -> Result<(), String> {
    let ambiguous = db::get_ambiguous_orders(&state.pool).await?;

    let now = chrono::Utc::now();

    for order in &ambiguous {
        let age = now - order.updated_at;
        if age < chrono::Duration::from_std(STALE_THRESHOLD).unwrap_or(chrono::Duration::seconds(30)) {
            continue; // Not stale yet
        }

        debug!(
            order_id = order.id,
            state = %order.state,
            age_secs = age.num_seconds(),
            "resolving stale order"
        );

        match state
            .exchange
            .get_order_by_client_id(order.client_order_id)
            .await
        {
            Ok(exchange_status) => {
                // Use shared resolution logic
                let new_state = match state::resolve_exchange_state(&order.state, &exchange_status.status) {
                    Some(s) => Some(s),
                    None => {
                        // Special case: PendingCancel + Resting → re-send cancel
                        if order.state == OrderState::PendingCancel
                            && exchange_status.status == ExchangeOrderState::Resting
                        {
                            if let Some(eid) = &order.exchange_order_id {
                                warn!(order_id = order.id, "re-sending cancel");
                                let _ = state.exchange.cancel_order(eid).await;
                            }
                        } else {
                            warn!(
                                order_id = order.id,
                                local_state = %order.state,
                                exchange_state = ?exchange_status.status,
                                "unhandled reconciliation case"
                            );
                        }
                        None
                    }
                };

                if let Some(new_state) = new_state {
                    info!(
                        order_id = order.id,
                        from = %order.state,
                        to = %new_state,
                        "reconciliation resolved order"
                    );

                    if let Err(e) = db::update_order_state(
                        &state.pool,
                        order.id,
                        new_state,
                        Some(&exchange_status.exchange_order_id),
                        Some(exchange_status.filled_quantity),
                        None,
                    )
                    .await
                    {
                        error!(error = %e, "failed to update reconciled state");
                    }
                }
            }
            Err(harman::error::ExchangeError::NotFound(_)) => {
                // Order not found on exchange
                if order.state == OrderState::Submitted {
                    info!(
                        order_id = order.id,
                        "submitted order not found on exchange, marking rejected"
                    );
                    let _ = db::update_order_state(
                        &state.pool,
                        order.id,
                        OrderState::Rejected,
                        None,
                        None,
                        None,
                    )
                    .await;
                }
            }
            Err(e) => {
                warn!(
                    error = %e,
                    order_id = order.id,
                    "exchange query failed during reconciliation"
                );
            }
        }
    }

    Ok(())
}
