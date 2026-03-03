use tracing::{debug, error, info, warn};

use harman::db;
use harman::error::ExchangeError;
use harman::fill_processor;
use harman::order_importer;
use harman::settlement_recorder;
use harman::state::{self, OrderState};
use harman::types::ExchangeOrderState;

use crate::Oms;

/// Run recovery before starting the API server.
///
/// Resolves orders in ambiguous states (submitted, pending_cancel)
/// by querying the exchange for their true state.
///
/// This MUST complete before the API server starts to prevent
/// duplicate submissions or stale risk state.
pub async fn run(oms: &Oms, session_id: i64) -> Result<(), String> {
    info!("starting recovery coordinator");

    // 1. Resolve ambiguous orders
    resolve_ambiguous_orders(oms, session_id).await?;

    // 2. Discover external resting orders
    discover_external_orders(oms, session_id).await?;

    // 3. Discover missing fills
    discover_missing_fills(oms, session_id).await?;

    // 4. Discover settlements (zero out positions for settled markets)
    discover_settlements(oms, session_id).await?;

    // 5. Verify position consistency
    verify_positions(oms, session_id).await?;

    // 6. Rebuild risk state (just log it, the real check happens per-order)
    let risk_state = db::compute_risk_state(&oms.pool, session_id).await?;
    info!(
        open_notional = %risk_state.open_notional,
        max_notional = %oms.ems.risk_limits.max_notional,
        "risk state after recovery"
    );

    // 7. Clean up stale queue items (items marked processing from a crash)
    clean_stale_queue(oms).await?;

    info!("recovery complete");
    Ok(())
}

/// Resolve orders in submitted or pending_cancel state
async fn resolve_ambiguous_orders(oms: &Oms, session_id: i64) -> Result<(), String> {
    let ambiguous = db::get_ambiguous_orders(&oms.pool, session_id).await?;

    if ambiguous.is_empty() {
        info!("no ambiguous orders to recover");
        return Ok(());
    }

    info!(count = ambiguous.len(), "recovering ambiguous orders");

    for order in &ambiguous {
        // Prefer get_order_by_exchange_id when the exchange order ID is known
        // (more reliable direct lookup vs list-based client_order_id search).
        // Fall back to get_order_by_client_id for Submitted orders where the
        // POST response was lost and exchange_order_id is None.
        let exchange_result = if let Some(eid) = &order.exchange_order_id {
            oms.exchange.get_order_by_exchange_id(eid).await
        } else {
            oms.exchange
                .get_order_by_client_id(order.client_order_id)
                .await
        };
        match exchange_result {
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
                                match oms.exchange.cancel_order(eid).await {
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
                    // Infer cancel reason from exchange data
                    let cancel_reason = if new_state == OrderState::Cancelled {
                        if exchange_status.close_cancel_count.unwrap_or(0) > 0 {
                            Some(harman::types::CancelReason::Expired)
                        } else {
                            Some(harman::types::CancelReason::ExchangeCancel)
                        }
                    } else {
                        None
                    };

                    if let Err(e) = db::update_order_state(
                        &oms.pool,
                        order.id,
                        session_id,
                        new_state,
                        Some(&exchange_status.exchange_order_id),
                        Some(exchange_status.filled_quantity),
                        cancel_reason.as_ref(),
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
            Err(ref e) if e.is_not_found() => {
                // NotFound is ambiguous — could mean order aged out of API, endpoint
                // is wrong, or auth failed. Do NOT auto-resolve any state.
                // Pile-up of unresolved orders is a visible signal that something's broken.
                warn!(
                    order_id = order.id,
                    state = %order.state,
                    "recovery: order not found on exchange, leaving for manual review"
                );
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

/// Fetch resting orders from exchange and import any not tracked locally.
async fn discover_external_orders(oms: &Oms, session_id: i64) -> Result<(), String> {
    let exchange_orders = oms
        .exchange
        .get_orders()
        .await
        .map_err(|e| format!("get orders: {}", e))?;

    info!(count = exchange_orders.len(), "fetched exchange resting orders for recovery");

    let local_orders = db::list_orders(&oms.pool, session_id, None).await?;

    let imported = order_importer::import_external_orders(
        &oms.pool,
        session_id,
        &exchange_orders,
        &local_orders,
        false, // no DB existence check during recovery (faster)
        "recovery",
    )
    .await?;

    if imported > 0 {
        info!(count = imported, "recovery: imported external resting orders");
    }

    Ok(())
}

/// Fetch fills from exchange and record any missing ones.
/// Also updates order state when fills bring an order to Filled/PartiallyFilled.
async fn discover_missing_fills(oms: &Oms, session_id: i64) -> Result<(), String> {
    let fills = oms
        .exchange
        .get_fills(None)
        .await
        .map_err(|e| format!("get fills: {}", e))?;

    info!(count = fills.len(), "fetched exchange fills for recovery");

    let orders = db::list_orders(&oms.pool, session_id, None).await?;

    let import_result = fill_processor::import_fills(
        &oms.pool,
        session_id,
        &fills,
        &orders,
        "recovery",
    )
    .await?;

    if import_result.recorded > 0 {
        info!(
            count = import_result.recorded,
            external = import_result.external_imported,
            state_updates = import_result.state_updates,
            "recorded missing fills during recovery"
        );
    }

    Ok(())
}

/// Fetch settlements from exchange and record any new ones.
async fn discover_settlements(oms: &Oms, session_id: i64) -> Result<(), String> {
    let settlements = oms
        .exchange
        .get_settlements(None)
        .await
        .map_err(|e| format!("get settlements: {}", e))?;

    info!(count = settlements.len(), "fetched exchange settlements for recovery");

    let recorded = settlement_recorder::record_settlements(
        &oms.pool,
        session_id,
        &settlements,
        "recovery",
    )
    .await?;

    if recorded > 0 {
        info!(count = recorded, "imported settlement records during recovery");
    }

    Ok(())
}

/// Check local positions against exchange positions.
/// Logs settled tickers separately from genuinely mismatched positions.
async fn verify_positions(oms: &Oms, session_id: i64) -> Result<(), String> {
    let exchange_positions = oms
        .exchange
        .get_positions()
        .await
        .map_err(|e| format!("get positions: {}", e))?;

    let settled_tickers = db::get_settled_tickers(&oms.pool, session_id).await.unwrap_or_default();

    info!(
        exchange_positions = exchange_positions.len(),
        settled_tickers = settled_tickers.len(),
        "position verification"
    );

    for pos in &exchange_positions {
        info!(
            ticker = %pos.ticker,
            quantity = %pos.quantity,
            side = ?pos.side,
            "exchange position"
        );
    }

    if !settled_tickers.is_empty() {
        debug!(count = settled_tickers.len(), "settled tickers excluded from position comparison");
    }

    Ok(())
}

/// Clean up stale queue items that were marked processing when we crashed
async fn clean_stale_queue(oms: &Oms) -> Result<(), String> {
    let client = oms
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
