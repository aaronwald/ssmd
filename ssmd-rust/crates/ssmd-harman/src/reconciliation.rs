use serde::Serialize;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use harman::db;
use harman::state::{self, OrderState};
use harman::types::ExchangeOrderState;

use crate::AppState;

const STALE_THRESHOLD: Duration = Duration::from_secs(30);

#[derive(Debug, Serialize)]
pub struct ReconcileResult {
    pub fills_discovered: u64,
    pub orders_resolved: u64,
    pub errors: Vec<String>,
}

/// Run one full reconciliation cycle: discover fills, then resolve stale orders.
///
/// Called explicitly via `POST /v1/admin/reconcile` — no background polling.
pub async fn reconcile(state: &AppState) -> ReconcileResult {
    let mut result = ReconcileResult {
        fills_discovered: 0,
        orders_resolved: 0,
        errors: vec![],
    };

    match discover_fills(state).await {
        Ok(count) => result.fills_discovered = count,
        Err(e) => {
            error!(error = %e, "fill discovery failed");
            result.errors.push(format!("fill discovery: {}", e));
        }
    }

    match resolve_stale_orders(state).await {
        Ok(count) => result.orders_resolved = count,
        Err(e) => {
            error!(error = %e, "stale order resolution failed");
            result.errors.push(format!("stale resolution: {}", e));
        }
    }

    info!(
        fills_discovered = result.fills_discovered,
        orders_resolved = result.orders_resolved,
        errors = result.errors.len(),
        "reconciliation complete"
    );

    result
}

/// Fetch recent fills from exchange and record any missing ones.
///
/// Loads all orders once and builds a lookup to avoid N+1 queries.
async fn discover_fills(state: &AppState) -> Result<u64, String> {
    let fills = state
        .exchange
        .get_fills()
        .await
        .map_err(|e| format!("get fills: {}", e))?;

    debug!(count = fills.len(), "fetched exchange fills");

    let orders = db::list_orders(&state.pool, state.session_id, None).await?;

    let mut count = 0u64;
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
                count += 1;
            }
        }
    }

    Ok(count)
}

/// Find and resolve orders stuck in ambiguous states.
async fn resolve_stale_orders(state: &AppState) -> Result<u64, String> {
    let ambiguous = db::get_ambiguous_orders(&state.pool, state.session_id).await?;

    let now = chrono::Utc::now();
    let mut count = 0u64;

    for order in &ambiguous {
        let age = now - order.updated_at;
        if age
            < chrono::Duration::from_std(STALE_THRESHOLD)
                .unwrap_or(chrono::Duration::seconds(30))
        {
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
                let new_state =
                    match state::resolve_exchange_state(&order.state, &exchange_status.status) {
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
                        "reconciliation",
                    )
                    .await
                    {
                        error!(error = %e, "failed to update reconciled state");
                    } else {
                        count += 1;
                    }
                }
            }
            Err(harman::error::ExchangeError::NotFound(_)) => {
                if order.state == OrderState::Submitted {
                    info!(
                        order_id = order.id,
                        "submitted order not found on exchange, marking rejected"
                    );
                    if let Err(e) = db::update_order_state(
                        &state.pool,
                        order.id,
                        OrderState::Rejected,
                        None,
                        None,
                        None,
                        "reconciliation",
                    )
                    .await
                    {
                        error!(error = %e, "failed to update rejected state");
                    } else {
                        count += 1;
                    }
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

    Ok(count)
}
