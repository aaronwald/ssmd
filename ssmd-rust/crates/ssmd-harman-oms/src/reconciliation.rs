use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use harman::db;
use harman::fill_processor;
use harman::order_importer;
use harman::settlement_recorder;
use harman::state::{self, OrderState};
use harman::types::{Action, ExchangeOrderState, Side};
use rust_decimal::Decimal;

use crate::Oms;

const STALE_THRESHOLD: Duration = Duration::from_secs(30);

/// Thresholds for position mismatch severity.
const LARGE_MISMATCH_CONTRACTS: i64 = 1;
const LARGE_MISMATCH_NOTIONAL: &str = "10"; // dollars

#[derive(Debug, Serialize)]
pub struct ReconcileResult {
    pub settlements_discovered: u64,
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
pub async fn reconcile(oms: &Oms, session_id: i64) -> ReconcileResult {
    let start = Instant::now();

    let mut result = ReconcileResult {
        settlements_discovered: 0,
        fills_discovered: 0,
        orders_resolved: 0,
        position_mismatches: vec![],
        suspended: false,
        errors: vec![],
    };

    // Discover settlements first (needed for position comparison and cancel reason inference)
    let settled_tickers = match discover_settlements(oms, session_id).await {
        Ok(count) => {
            result.settlements_discovered = count;
            if count > 0 {
                oms.metrics.reconciliation_settlements_discovered.inc_by(count);
            }
            // Load the full set of settled tickers for downstream use
            db::get_settled_tickers(&oms.pool, session_id).await.unwrap_or_default()
        }
        Err(e) => {
            error!(error = %e, "settlement discovery failed");
            result.errors.push(format!("settlement discovery: {}", e));
            std::collections::HashSet::new()
        }
    };

    match discover_external_orders(oms, session_id).await {
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

    match discover_fills(oms, session_id).await {
        Ok(count) => {
            result.fills_discovered = count;
            oms.metrics.reconciliation_fills_discovered.inc_by(count);
        }
        Err(e) => {
            error!(error = %e, "fill discovery failed");
            result.errors.push(format!("fill discovery: {}", e));
        }
    }

    match resolve_stale_orders(oms, session_id, &settled_tickers).await {
        Ok(count) => result.orders_resolved = count,
        Err(e) => {
            error!(error = %e, "stale order resolution failed");
            result.errors.push(format!("stale resolution: {}", e));
        }
    }

    match compare_positions(oms, session_id, &settled_tickers).await {
        Ok(mismatches) => {
            result.position_mismatches = mismatches;
        }
        Err(e) => {
            error!(error = %e, "position comparison failed");
            result.errors.push(format!("position comparison: {}", e));
        }
    }

    // Check if any mismatch triggered suspension for this session
    result.suspended = oms.suspended_sessions.contains_key(&session_id);

    // Record metrics
    let elapsed = start.elapsed().as_secs_f64();
    oms.metrics.reconciliation_duration.observe(elapsed);

    if result.errors.is_empty() {
        oms.metrics.reconciliation_ok.inc();
        oms.metrics
            .reconciliation_last_success
            .set(chrono::Utc::now().timestamp());
    }

    info!(
        settlements_discovered = result.settlements_discovered,
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

/// Fetch settlements from exchange and record any new ones in the DB.
///
/// Returns the count of newly discovered settlements.
async fn discover_settlements(oms: &Oms, session_id: i64) -> Result<u64, String> {
    let settlements = oms
        .exchange
        .get_settlements(None)
        .await
        .map_err(|e| format!("get settlements: {}", e))?;

    debug!(count = settlements.len(), "fetched exchange settlements");

    settlement_recorder::record_settlements(&oms.pool, session_id, &settlements, "reconciliation")
        .await
}

/// Fetch resting orders from exchange and import any not tracked locally.
///
/// External resting orders (placed via exchange website) are imported as
/// synthetic orders in 'acknowledged' state into the user's session.
/// With stable sessions, one harman instance = one exchange account.
async fn discover_external_orders(oms: &Oms, session_id: i64) -> Result<u64, String> {
    let exchange_orders = oms
        .exchange
        .get_orders()
        .await
        .map_err(|e| format!("get orders: {}", e))?;

    debug!(count = exchange_orders.len(), "fetched exchange resting orders");

    let local_orders = db::list_orders(&oms.pool, session_id, None).await?;

    let count = order_importer::import_external_orders(
        &oms.pool,
        session_id,
        &exchange_orders,
        &local_orders,
        true, // check_db_exists for reconciliation
        "reconciliation",
    )
    .await?;

    if count > 0 {
        oms.metrics.fills_external_imported.inc_by(count);
    }

    Ok(count)
}

/// Fetch recent fills from exchange and record any missing ones.
/// After recording fills, update order states (Acknowledged → Filled/PartiallyFilled).
///
/// With stable sessions, all fills for this exchange account belong to the user's session.
/// No cross-session lookup needed.
async fn discover_fills(oms: &Oms, session_id: i64) -> Result<u64, String> {
    let fills = oms
        .exchange
        .get_fills(None)
        .await
        .map_err(|e| format!("get fills: {}", e))?;

    debug!(count = fills.len(), "fetched exchange fills");

    let session_orders = db::list_orders(&oms.pool, session_id, None).await?;

    let import_result = fill_processor::import_fills(
        &oms.pool,
        session_id,
        &fills,
        &session_orders,
        "reconciliation",
    )
    .await?;

    // Update metrics
    let total = import_result.recorded;
    if total > 0 {
        oms.ems.metrics.fills_recorded.inc_by(total);
    }
    if import_result.external_imported > 0 {
        oms.metrics
            .fills_external_imported
            .inc_by(import_result.external_imported);
    }

    Ok(total)
}

/// Compare local positions (from filled orders) against exchange positions.
///
/// Aggregates across ALL sessions for this exchange (global view) to avoid
/// phantom mismatches from per-session vs global exchange comparison.
/// Tickers with settlement records are skipped — their local fills sum to non-zero
/// but the exchange reports zero (positions disappear after settlement).
async fn compare_positions(
    oms: &Oms,
    session_id: i64,
    settled_tickers: &HashSet<String>,
) -> Result<Vec<PositionMismatch>, String> {
    let exchange_positions = oms
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
    let orders = db::list_orders(&oms.pool, session_id, None).await?;
    let mut local_map: HashMap<String, Decimal> = HashMap::new();
    for order in &orders {
        if order.filled_quantity <= Decimal::ZERO {
            continue;
        }
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
        // Skip settled tickers — exchange positions disappear after settlement
        // while local fills remain (expected mismatch, not a real problem)
        if settled_tickers.contains(ticker) {
            debug!(ticker = %ticker, "skipping settled ticker in position comparison");
            continue;
        }

        let local_qty = local_map.get(ticker).copied().unwrap_or(Decimal::ZERO);
        let exchange_qty = exchange_map.get(ticker).copied().unwrap_or(Decimal::ZERO);
        let diff = (local_qty - exchange_qty).abs();

        if diff.is_zero() {
            continue;
        }

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

        oms.metrics
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
/// Uses settled_tickers to infer cancel reason when exchange auto-cancels resting orders
/// on market settlement (Expired) vs unknown exchange cancels (ExchangeCancel).
async fn resolve_stale_orders(
    oms: &Oms,
    session_id: i64,
    settled_tickers: &HashSet<String>,
) -> Result<u64, String> {
    let ambiguous = db::get_ambiguous_orders(&oms.pool, session_id).await?;

    let now = chrono::Utc::now();
    let mut count = 0u64;

    for order in &ambiguous {
        let age = now - order.updated_at;
        if age
            < chrono::Duration::from_std(STALE_THRESHOLD)
                .unwrap_or(chrono::Duration::seconds(30))
        {
            continue;
        }

        debug!(
            order_id = order.id,
            state = %order.state,
            age_secs = age.num_seconds(),
            "resolving stale order"
        );

        // Prefer get_order_by_exchange_id when the exchange order ID is known
        let exchange_result = if let Some(eid) = &order.exchange_order_id {
            oms.exchange.get_order_by_exchange_id(eid).await
        } else {
            oms.exchange
                .get_order_by_client_id(order.client_order_id)
                .await
        };
        match exchange_result {
            Ok(exchange_status) => {
                let new_state =
                    match state::resolve_exchange_state(&order.state, &exchange_status.status) {
                        Some(s) => Some(s),
                        None => {
                            if order.state == OrderState::PendingCancel
                                && exchange_status.status == ExchangeOrderState::Resting
                            {
                                // Re-enqueue cancel through the EMS pump path instead of
                                // firing a REST mutation inline. This gives audit trail,
                                // retry logic, and rate limiting.
                                warn!(order_id = order.id, "re-enqueuing cancel via EMS queue");
                                if let Err(e) = db::enqueue_cancel(&oms.pool, order.id, "reconciliation").await {
                                    error!(error = %e, order_id = order.id, "failed to re-enqueue cancel");
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
                    // Infer cancel reason:
                    // 1. If exchange reports close_cancel_count > 0, it's a market-close cancel (Expired)
                    // 2. Fall back to settled_tickers heuristic for NotFound cases
                    // 3. Otherwise it's an unknown exchange cancel (ExchangeCancel)
                    let cancel_reason = if new_state == OrderState::Cancelled {
                        if exchange_status.close_cancel_count.unwrap_or(0) > 0
                            || settled_tickers.contains(&order.ticker)
                        {
                            Some(harman::types::CancelReason::Expired)
                        } else {
                            Some(harman::types::CancelReason::ExchangeCancel)
                        }
                    } else {
                        None
                    };

                    info!(
                        order_id = order.id,
                        from = %order.state,
                        to = %new_state,
                        cancel_reason = ?cancel_reason,
                        "reconciliation resolved order"
                    );

                    if let Err(e) = db::update_order_state(
                        &oms.pool,
                        order.id,
                        session_id,
                        new_state,
                        Some(&exchange_status.exchange_order_id),
                        Some(exchange_status.filled_quantity),
                        cancel_reason.as_ref(),
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
            Err(ref e) if e.is_not_found() => {
                // Single-order GET returned 404 — fall back to contextual data.
                //
                // The demo API's GET /portfolio/orders/{id} returns 404 for settled/cancelled
                // orders, so we use settled_tickers (from settlement discovery) to infer the
                // order's final state when the direct endpoint fails.
                if settled_tickers.contains(&order.ticker) {
                    // Market settled → exchange auto-cancelled resting orders at settlement.
                    let cancel_reason = harman::types::CancelReason::Expired;
                    info!(
                        order_id = order.id,
                        ticker = %order.ticker,
                        from = %order.state,
                        to = %OrderState::Cancelled,
                        cancel_reason = ?cancel_reason,
                        "reconciliation: order not found but ticker settled, resolving as expired"
                    );
                    if let Err(e) = db::update_order_state(
                        &oms.pool,
                        order.id,
                        session_id,
                        OrderState::Cancelled,
                        order.exchange_order_id.as_deref(),
                        Some(order.filled_quantity),
                        Some(&cancel_reason),
                        "reconciliation",
                    )
                    .await
                    {
                        error!(error = %e, order_id = order.id, "failed to update settled-cancel state");
                    } else {
                        count += 1;
                    }
                } else {
                    // Not settled — genuinely ambiguous. Leave for review.
                    warn!(
                        order_id = order.id,
                        state = %order.state,
                        ticker = %order.ticker,
                        "reconciliation: order not found on exchange and ticker not settled, leaving for review"
                    );
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
