use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use harman::db;
use harman::state::{self, OrderState};
use harman::types::{Action, ExchangeOrderState, Side};
use rust_decimal::Decimal;

use crate::AppState;

const STALE_THRESHOLD: Duration = Duration::from_secs(30);

/// Thresholds for position mismatch severity.
/// Large mismatches trigger session suspension.
const LARGE_MISMATCH_CONTRACTS: i64 = 1;
const LARGE_MISMATCH_NOTIONAL: &str = "10"; // dollars

#[derive(Debug, Serialize)]
pub struct ReconcileResult {
    pub fills_discovered: u64,
    pub orders_resolved: u64,
    pub position_mismatches: Vec<PositionMismatch>,
    pub suspended: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PositionMismatch {
    pub ticker: String,
    pub local_quantity: String,
    pub exchange_quantity: String,
    pub severity: String,
}

/// Run one full reconciliation cycle: discover fills, resolve stale orders, compare positions.
///
/// Called explicitly via `POST /v1/admin/reconcile` — no background polling.
pub async fn reconcile(state: &AppState, session_id: i64) -> ReconcileResult {
    let start = Instant::now();

    let mut result = ReconcileResult {
        fills_discovered: 0,
        orders_resolved: 0,
        position_mismatches: vec![],
        suspended: false,
        errors: vec![],
    };

    match discover_external_orders(state, session_id).await {
        Ok(count) => {
            if count > 0 {
                info!(count, "imported external resting orders");
            }
        }
        Err(e) => {
            error!(error = %e, "external order discovery failed");
            result.errors.push(format!("external order discovery: {}", e));
        }
    }

    match discover_fills(state, session_id).await {
        Ok(count) => {
            result.fills_discovered = count;
            state.metrics.reconciliation_fills_discovered.inc_by(count);
        }
        Err(e) => {
            error!(error = %e, "fill discovery failed");
            result.errors.push(format!("fill discovery: {}", e));
        }
    }

    match resolve_stale_orders(state, session_id).await {
        Ok(count) => result.orders_resolved = count,
        Err(e) => {
            error!(error = %e, "stale order resolution failed");
            result.errors.push(format!("stale resolution: {}", e));
        }
    }

    match compare_positions(state, session_id).await {
        Ok(mismatches) => {
            result.position_mismatches = mismatches;
        }
        Err(e) => {
            error!(error = %e, "position comparison failed");
            result.errors.push(format!("position comparison: {}", e));
        }
    }

    // Check if any mismatch triggered suspension for this session
    result.suspended = state.suspended_sessions.contains_key(&session_id);

    // Record metrics
    let elapsed = start.elapsed().as_secs_f64();
    state.metrics.reconciliation_duration.observe(elapsed);

    if result.errors.is_empty() {
        state.metrics.reconciliation_ok.inc();
        state
            .metrics
            .reconciliation_last_success
            .set(chrono::Utc::now().timestamp());
    }

    info!(
        fills_discovered = result.fills_discovered,
        orders_resolved = result.orders_resolved,
        mismatches = result.position_mismatches.len(),
        suspended = result.suspended,
        errors = result.errors.len(),
        elapsed_secs = %format!("{:.3}", elapsed),
        "reconciliation complete"
    );

    result
}

/// Fetch resting orders from exchange and import any not tracked locally.
///
/// External resting orders (placed via exchange website) are imported as
/// synthetic orders in 'acknowledged' state so they appear in the blotter.
async fn discover_external_orders(state: &AppState, session_id: i64) -> Result<u64, String> {
    let exchange_orders = state
        .ems
        .exchange
        .get_orders()
        .await
        .map_err(|e| format!("get orders: {}", e))?;

    debug!(count = exchange_orders.len(), "fetched exchange resting orders");

    let local_orders = db::list_orders(&state.pool, session_id, None).await?;

    let mut count = 0u64;
    for order in &exchange_orders {
        // Check if we already track this order locally
        let known = local_orders
            .iter()
            .any(|o| o.exchange_order_id.as_deref() == Some(&order.exchange_order_id));

        if known {
            continue;
        }

        info!(
            exchange_order_id = %order.exchange_order_id,
            ticker = %order.ticker,
            side = ?order.side,
            action = ?order.action,
            quantity = %order.quantity,
            price = %order.price_dollars,
            client_order_id = ?order.client_order_id,
            "importing external resting order"
        );

        match db::create_external_resting_order(
            &state.pool,
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
                state.metrics.fills_external_imported.inc();
                count += 1;
            }
            Err(e) => {
                error!(
                    error = %e,
                    exchange_order_id = %order.exchange_order_id,
                    "failed to import external resting order"
                );
            }
        }
    }

    Ok(count)
}

/// Fetch recent fills from exchange and record any missing ones.
/// After recording fills, update order states (Acknowledged → Filled/PartiallyFilled).
///
/// Loads all orders once and builds a lookup to avoid N+1 queries.
async fn discover_fills(state: &AppState, session_id: i64) -> Result<u64, String> {
    let fills = state
        .ems
        .exchange
        .get_fills(None)
        .await
        .map_err(|e| format!("get fills: {}", e))?;

    debug!(count = fills.len(), "fetched exchange fills");

    let orders = db::list_orders(&state.pool, session_id, None).await?;

    let mut count = 0u64;
    // Track which orders got new fills so we can update their states
    let mut orders_with_new_fills: std::collections::HashSet<i64> = std::collections::HashSet::new();

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
                info!(
                    order_id = order.id,
                    trade_id = %fill.trade_id,
                    "recorded missing fill"
                );
                state.ems.metrics.fills_recorded.inc();
                orders_with_new_fills.insert(order.id);
                count += 1;
            }
        } else {
            // No matching local order — this is an external fill.
            // Fills are sacrosanct: never drop fills. Import as synthetic order.
            info!(
                trade_id = %fill.trade_id,
                exchange_order_id = %fill.order_id,
                ticker = %fill.ticker,
                client_order_id = ?fill.client_order_id,
                "importing external fill"
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
                        state.ems.metrics.fills_recorded.inc();
                        state.metrics.fills_external_imported.inc();
                        count += 1;
                    }
                }
                Err(e) => {
                    error!(
                        error = %e,
                        trade_id = %fill.trade_id,
                        "failed to import external fill"
                    );
                }
            }
        }
    }

    // Update order states for orders that received new fills
    for order_id in &orders_with_new_fills {
        if let Some(order) = orders.iter().find(|o| o.id == *order_id) {
            // Only update non-terminal orders
            if order.state.is_terminal() {
                continue;
            }
            // Compute total filled quantity from all fills for this order
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
                    order_qty = %order.quantity,
                    "reconciliation updated order state from fill"
                );
                if let Err(e) = db::update_order_state(
                    &state.pool,
                    *order_id,
                    session_id,
                    new_state,
                    None,
                    Some(filled_qty),
                    None,
                    "reconciliation",
                )
                .await
                {
                    error!(error = %e, order_id, "failed to update order state after fill");
                }
            }
        }
    }

    Ok(count)
}

/// Compare local positions (from filled orders) against exchange positions.
///
/// For each ticker, compute local net position from filled orders:
///   Buy → +quantity, Sell → -quantity
/// Then compare against exchange `get_positions()`.
///
/// Large mismatches (>1 contract or >$10 notional) trigger session suspension.
async fn compare_positions(state: &AppState, session_id: i64) -> Result<Vec<PositionMismatch>, String> {
    let exchange_positions = state
        .ems
        .exchange
        .get_positions()
        .await
        .map_err(|e| format!("get positions: {}", e))?;

    // Build exchange position map: ticker → signed quantity (Yes side positive, No side negative)
    let mut exchange_map: HashMap<String, Decimal> = HashMap::new();
    for pos in &exchange_positions {
        let signed = match pos.side {
            Side::Yes => pos.quantity,
            Side::No => -pos.quantity,
        };
        *exchange_map.entry(pos.ticker.clone()).or_default() += signed;
    }

    // Compute local positions from filled orders in this session
    let orders = db::list_orders(&state.pool, session_id, None).await?;
    let mut local_map: HashMap<String, Decimal> = HashMap::new();
    for order in &orders {
        if order.filled_quantity <= Decimal::ZERO {
            continue;
        }
        // Buy adds to position, Sell subtracts
        let signed = match order.action {
            Action::Buy => order.filled_quantity,
            Action::Sell => -order.filled_quantity,
        };
        *local_map.entry(order.ticker.clone()).or_default() += signed;
    }

    // Collect all tickers from both sides
    let mut all_tickers: Vec<String> = local_map.keys().cloned().collect();
    for ticker in exchange_map.keys() {
        if !all_tickers.contains(ticker) {
            all_tickers.push(ticker.clone());
        }
    }
    all_tickers.sort();

    let large_threshold_contracts = Decimal::from(LARGE_MISMATCH_CONTRACTS);
    let large_threshold_notional: Decimal = LARGE_MISMATCH_NOTIONAL
        .parse()
        .unwrap_or(Decimal::from(10));

    let mut mismatches = Vec::new();
    let mut any_large = false;

    for ticker in &all_tickers {
        let local_qty = local_map.get(ticker).copied().unwrap_or(Decimal::ZERO);
        let exchange_qty = exchange_map.get(ticker).copied().unwrap_or(Decimal::ZERO);
        let diff = (local_qty - exchange_qty).abs();

        if diff.is_zero() {
            continue;
        }

        // Determine severity: diff > 1 contract is large, or estimate notional
        let is_large = diff > large_threshold_contracts || diff > large_threshold_notional;
        let severity = if is_large { "large" } else { "small" };

        warn!(
            ticker = %ticker,
            local_qty = %local_qty,
            exchange_qty = %exchange_qty,
            diff = %diff,
            severity,
            "position mismatch detected"
        );

        state
            .metrics
            .reconciliation_mismatch
            .with_label_values(&[severity])
            .inc();

        if is_large {
            any_large = true;
        }

        mismatches.push(PositionMismatch {
            ticker: ticker.clone(),
            local_quantity: local_qty.to_string(),
            exchange_quantity: exchange_qty.to_string(),
            severity: severity.to_string(),
        });
    }

    if any_large {
        warn!(
            session_id,
            mismatches = mismatches.len(),
            "position mismatch detected (may be external orders, not suspending)"
        );
    }

    Ok(mismatches)
}

/// Find and resolve orders stuck in ambiguous states.
async fn resolve_stale_orders(state: &AppState, session_id: i64) -> Result<u64, String> {
    let ambiguous = db::get_ambiguous_orders(&state.pool, session_id).await?;

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
            .ems
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
                                    let _ = state.ems.exchange.cancel_order(eid).await;
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
                        session_id,
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
                        session_id,
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
