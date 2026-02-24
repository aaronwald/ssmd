use std::sync::Arc;
use tracing::{error, info, warn};

use harman::db;
use harman::error::ExchangeError;
use harman::state::{self, OrderState};
use harman::types::ExchangeOrderState;

use crate::AppState;

/// Run recovery before starting the sweeper.
///
/// Resolves orders in ambiguous states (submitted, pending_cancel)
/// by querying the exchange for their true state.
///
/// This MUST complete before the sweeper starts to prevent
/// duplicate submissions.
pub async fn run(state: &Arc<AppState>) -> Result<(), String> {
    info!("starting recovery coordinator");

    // 1. Resolve ambiguous orders
    resolve_ambiguous_orders(state).await?;

    // 2. Discover missing fills
    discover_missing_fills(state).await?;

    // 3. Verify position consistency
    verify_positions(state).await?;

    // 4. Rebuild risk state (just log it, the real check happens per-order)
    let risk_state = db::compute_risk_state(&state.pool, 1).await?;
    info!(
        open_notional = %risk_state.open_notional,
        max_notional = %state.risk_limits.max_notional,
        "risk state after recovery"
    );

    // 5. Clean up stale queue items (items marked processing from a crash)
    clean_stale_queue(state).await?;

    info!("recovery complete");
    Ok(())
}

/// Resolve orders in submitted or pending_cancel state
async fn resolve_ambiguous_orders(state: &Arc<AppState>) -> Result<(), String> {
    let ambiguous = db::get_ambiguous_orders(&state.pool).await?;

    if ambiguous.is_empty() {
        info!("no ambiguous orders to recover");
        return Ok(());
    }

    info!(count = ambiguous.len(), "recovering ambiguous orders");

    for order in &ambiguous {
        match state
            .exchange
            .get_order_by_client_id(order.client_order_id)
            .await
        {
            Ok(exchange_status) => {
                // Use shared resolution logic
                let new_state = match state::resolve_exchange_state(&order.state, &exchange_status.status) {
                    Some(s) => {
                        info!(
                            order_id = order.id,
                            from = %order.state,
                            to = %s,
                            "recovery resolved order"
                        );
                        Some(s)
                    }
                    None => {
                        // Special case: PendingCancel + Resting → re-send cancel
                        if order.state == OrderState::PendingCancel
                            && exchange_status.status == ExchangeOrderState::Resting
                        {
                            warn!(
                                order_id = order.id,
                                "recovery: pending_cancel still resting, re-sending cancel"
                            );
                            if let Some(eid) = &order.exchange_order_id {
                                match state.exchange.cancel_order(eid).await {
                                    Ok(()) => {
                                        info!(order_id = order.id, "re-cancel succeeded");
                                        Some(OrderState::Cancelled)
                                    }
                                    Err(e) => {
                                        warn!(
                                            error = %e,
                                            order_id = order.id,
                                            "re-cancel failed, will retry next reconciliation"
                                        );
                                        None
                                    }
                                }
                            } else {
                                None
                            }
                        } else {
                            warn!(
                                order_id = order.id,
                                local = %order.state,
                                exchange = ?exchange_status.status,
                                "unhandled recovery case"
                            );
                            None
                        }
                    }
                };

                if let Some(new_state) = new_state {
                    if let Err(e) = db::update_order_state(
                        &state.pool,
                        order.id,
                        new_state,
                        Some(&exchange_status.exchange_order_id),
                        Some(exchange_status.filled_quantity),
                        None,
                        "recovery",
                    )
                    .await
                    {
                        error!(
                            error = %e,
                            order_id = order.id,
                            "failed to update recovered order state"
                        );
                    }
                }
            }
            Err(ExchangeError::NotFound(_)) => {
                if order.state == OrderState::Submitted {
                    info!(
                        order_id = order.id,
                        "recovery: submitted order not found on exchange → rejected"
                    );
                    let _ = db::update_order_state(
                        &state.pool,
                        order.id,
                        OrderState::Rejected,
                        None,
                        None,
                        None,
                        "recovery",
                    )
                    .await;
                } else {
                    info!(
                        order_id = order.id,
                        "recovery: pending_cancel order not found → cancelled"
                    );
                    let _ = db::update_order_state(
                        &state.pool,
                        order.id,
                        OrderState::Cancelled,
                        None,
                        None,
                        None,
                        "recovery",
                    )
                    .await;
                }
            }
            Err(ExchangeError::Connection(_) | ExchangeError::Timeout { .. }) => {
                error!(
                    order_id = order.id,
                    "exchange unreachable during recovery, cannot resolve - exiting"
                );
                return Err("exchange unreachable during recovery".to_string());
            }
            Err(e) => {
                error!(
                    error = %e,
                    order_id = order.id,
                    "exchange error during recovery"
                );
                return Err(format!("exchange error during recovery: {}", e));
            }
        }
    }

    Ok(())
}

/// Fetch fills from exchange and record any missing ones.
///
/// Loads all orders once to avoid N+1 queries.
async fn discover_missing_fills(state: &Arc<AppState>) -> Result<(), String> {
    let fills = state
        .exchange
        .get_fills()
        .await
        .map_err(|e| format!("get fills: {}", e))?;

    info!(count = fills.len(), "fetched exchange fills for recovery");

    let orders = db::list_orders(&state.pool, None).await?;
    let mut recorded = 0;

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
                recorded += 1;
            }
        }
    }

    if recorded > 0 {
        info!(count = recorded, "recorded missing fills during recovery");
    }

    Ok(())
}

/// Check local positions against exchange positions
async fn verify_positions(state: &Arc<AppState>) -> Result<(), String> {
    let exchange_positions = state
        .exchange
        .get_positions()
        .await
        .map_err(|e| format!("get positions: {}", e))?;

    info!(
        count = exchange_positions.len(),
        "fetched exchange positions for verification"
    );

    // For MVP, just log any positions. A more sophisticated check would
    // compare against aggregated fills in our DB.
    for pos in &exchange_positions {
        info!(
            ticker = %pos.ticker,
            quantity = pos.quantity,
            side = ?pos.side,
            "exchange position"
        );
    }

    Ok(())
}

/// Clean up stale queue items that were marked processing when we crashed
async fn clean_stale_queue(state: &Arc<AppState>) -> Result<(), String> {
    let client = state
        .pool
        .get()
        .await
        .map_err(|e| format!("pool error: {}", e))?;

    let count = client
        .execute(
            "UPDATE order_queue SET processing = FALSE WHERE processing = TRUE",
            &[],
        )
        .await
        .map_err(|e| format!("clean stale queue: {}", e))?;

    if count > 0 {
        info!(count, "reset stale processing queue items");
    }

    Ok(())
}
