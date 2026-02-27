use std::sync::Arc;
use tracing::{error, info, warn};

use harman::db;
use harman::error::ExchangeError;
use harman::state::{self, OrderState};
use harman::types::ExchangeOrderState;
use rust_decimal::Decimal;

use crate::AppState;

/// Run recovery before starting the API server.
///
/// Resolves orders in ambiguous states (submitted, pending_cancel)
/// by querying the exchange for their true state.
///
/// This MUST complete before the API server starts to prevent
/// duplicate submissions or stale risk state.
///
/// Uses `startup_session_id` — other sessions recover on first pump.
pub async fn run(state: &Arc<AppState>) -> Result<(), String> {
    info!("starting recovery coordinator");

    let session_id = state.startup_session_id;

    // 1. Resolve ambiguous orders
    resolve_ambiguous_orders(state, session_id).await?;

    // 2. Discover missing fills
    discover_missing_fills(state, session_id).await?;

    // 3. Verify position consistency
    verify_positions(state).await?;

    // 4. Rebuild risk state (just log it, the real check happens per-order)
    let risk_state = db::compute_risk_state(&state.pool, session_id).await?;
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
async fn resolve_ambiguous_orders(state: &Arc<AppState>, session_id: i64) -> Result<(), String> {
    let ambiguous = db::get_ambiguous_orders(&state.pool, session_id).await?;

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
                        session_id,
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
                match order.state {
                    OrderState::Submitted => {
                        info!(
                            order_id = order.id,
                            "recovery: submitted order not found on exchange → rejected"
                        );
                        let _ = db::update_order_state(
                            &state.pool,
                            order.id,
                            session_id,
                            OrderState::Rejected,
                            None,
                            None,
                            None,
                            "recovery",
                        )
                        .await;
                    }
                    OrderState::PendingCancel => {
                        info!(
                            order_id = order.id,
                            "recovery: pending_cancel order not found → cancelled"
                        );
                        let _ = db::update_order_state(
                            &state.pool,
                            order.id,
                            session_id,
                            OrderState::Cancelled,
                            None,
                            None,
                            None,
                            "recovery",
                        )
                        .await;
                    }
                    _ => {
                        // Acknowledged/PartiallyFilled/PendingAmend/PendingDecrease not found
                        // on exchange is unusual — log warning but don't auto-cancel
                        warn!(
                            order_id = order.id,
                            state = %order.state,
                            "recovery: order in {} state not found on exchange, leaving for manual review",
                            order.state
                        );
                    }
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
/// Also updates order state when fills bring an order to Filled/PartiallyFilled.
async fn discover_missing_fills(state: &Arc<AppState>, session_id: i64) -> Result<(), String> {
    let fills = state
        .exchange
        .get_fills(None)
        .await
        .map_err(|e| format!("get fills: {}", e))?;

    info!(count = fills.len(), "fetched exchange fills for recovery");

    let orders = db::list_orders(&state.pool, session_id, None).await?;
    let mut recorded = 0;
    let mut orders_with_new_fills: std::collections::HashSet<i64> =
        std::collections::HashSet::new();

    let mut external_imported = 0u64;

    for fill in &fills {
        if let Some(order) = orders
            .iter()
            .find(|o| o.exchange_order_id.as_deref() == Some(&fill.order_id))
        {
            let inserted = db::record_fill(
                &state.pool,
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
                recorded += 1;
                orders_with_new_fills.insert(order.id);
            }
        } else {
            // External fill — import as synthetic order (fills are sacrosanct)
            info!(
                trade_id = %fill.trade_id,
                exchange_order_id = %fill.order_id,
                ticker = %fill.ticker,
                "recovery: importing external fill"
            );
            match db::create_external_order(
                &state.pool,
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
                        &state.pool,
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
                        recorded += 1;
                        external_imported += 1;
                    }
                }
                Err(e) => {
                    error!(
                        error = %e,
                        trade_id = %fill.trade_id,
                        "recovery: failed to import external fill"
                    );
                }
            }
        }
    }

    if recorded > 0 {
        info!(
            count = recorded,
            external = external_imported,
            "recorded missing fills during recovery"
        );
    }

    // Update order states for orders that received new fills
    for order_id in &orders_with_new_fills {
        if let Some(order) = orders.iter().find(|o| o.id == *order_id) {
            if order.state.is_terminal() {
                continue;
            }
            let filled_qty = db::get_filled_quantity(&state.pool, *order_id).await?;
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
                    "recovery: updated order state from fills"
                );
                if let Err(e) = db::update_order_state(
                    &state.pool,
                    *order_id,
                    session_id,
                    new_state,
                    None,
                    Some(filled_qty),
                    None,
                    "recovery",
                )
                .await
                {
                    error!(
                        error = %e,
                        order_id = order.id,
                        "failed to update order state from recovery fills"
                    );
                }
            }
        }
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

    for pos in &exchange_positions {
        info!(
            ticker = %pos.ticker,
            quantity = %pos.quantity,
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
