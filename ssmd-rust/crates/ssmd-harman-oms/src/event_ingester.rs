//! WebSocket event ingester.
//!
//! Consumes `ExchangeEvent` from an `EventStream` and routes events to shared
//! processors. This is the WS counterpart to REST-based reconciliation.
//!
//! Pure consumer — no direct exchange interaction, only DB writes via shared processors.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use deadpool_postgres::Pool;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use harman::audit::AuditSender;
use harman::db;
use harman::exchange::{ExchangeAdapter, ExchangeEvent};
use harman::fill_processor;
use harman::settlement_recorder;
use harman::state::OrderState;
use harman::types::{CancelReason, ExchangeFill};

use crate::OmsMetrics;
use crate::price_monitor::PriceMonitorHandle;
use crate::runner::PumpTrigger;

/// Ingests WS events and writes to DB via shared processors.
pub struct EventIngester {
    pool: Pool,
    exchange: Arc<dyn ExchangeAdapter>,
    metrics: Arc<OmsMetrics>,
    audit: AuditSender,
    pump_trigger: PumpTrigger,
    price_monitor: Option<PriceMonitorHandle>,
    /// Set to true when WS is connected, false on disconnect.
    pub ws_connected: Arc<AtomicBool>,
}

/// Result of processing events until the channel closes or disconnects.
pub struct IngestResult {
    pub fills_recorded: u64,
    pub orders_updated: u64,
    pub settlements_noted: u64,
    pub events_processed: u64,
}

impl EventIngester {
    pub fn new(
        pool: Pool,
        exchange: Arc<dyn ExchangeAdapter>,
        metrics: Arc<OmsMetrics>,
        audit: AuditSender,
        pump_trigger: PumpTrigger,
        price_monitor: Option<PriceMonitorHandle>,
    ) -> Self {
        Self {
            pool,
            exchange,
            metrics,
            audit,
            pump_trigger,
            price_monitor,
            ws_connected: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Run the ingester loop, consuming events from the broadcast receiver.
    ///
    /// Returns when the sender is dropped or an unrecoverable error occurs.
    pub async fn run(&self, mut rx: broadcast::Receiver<ExchangeEvent>) -> IngestResult {
        let mut result = IngestResult {
            fills_recorded: 0,
            orders_updated: 0,
            settlements_noted: 0,
            events_processed: 0,
        };

        loop {
            match rx.recv().await {
                Ok(event) => {
                    result.events_processed += 1;
                    self.handle_event(event, &mut result).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "event ingester lagged, missed events");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("event stream closed, ingester stopping");
                    break;
                }
            }
        }

        result
    }

    async fn handle_event(&self, event: ExchangeEvent, result: &mut IngestResult) {
        // Receiving any data event proves WS is connected. Handles the race where
        // the Connected event was broadcast before the ingester subscribed.
        if !matches!(event, ExchangeEvent::Disconnected { .. })
            && !self.ws_connected.load(Ordering::Relaxed)
        {
            info!("WS: marking connected (inferred from data event)");
            self.ws_connected.store(true, Ordering::Relaxed);
        }

        match event {
            ExchangeEvent::Fill {
                trade_id,
                exchange_order_id,
                ticker,
                side,
                action,
                price_dollars,
                quantity,
                is_taker,
                filled_at,
                client_order_id,
            } => {
                // Look up the order to get its session_id
                let order = match db::find_order_by_exchange_id(
                    &self.pool,
                    &exchange_order_id,
                )
                .await
                {
                    Ok(Some(o)) => Some(o),
                    Ok(None) => None,
                    Err(e) => {
                        error!(error = %e, "failed to look up order for fill");
                        None
                    }
                };

                let session_id_opt = order.as_ref().map(|o| o.session_id);

                self.audit.ws_event(
                    session_id_opt,
                    order.as_ref().map(|o| o.id),
                    "fill",
                    Some(serde_json::json!({
                        "trade_id": trade_id,
                        "exchange_order_id": exchange_order_id,
                        "ticker": ticker,
                        "price_dollars": price_dollars.to_string(),
                        "quantity": quantity.to_string(),
                    })),
                    None,
                );

                // Fill import requires a concrete session_id.
                // If the order is unknown (external fill), we can't import without a session.
                let Some(session_id) = session_id_opt else {
                    error!(exchange_order_id = %exchange_order_id, "fill for unknown order — no session to import into");
                    return;
                };

                let fill = ExchangeFill {
                    trade_id,
                    order_id: exchange_order_id,
                    ticker,
                    side,
                    action,
                    price_dollars,
                    quantity,
                    is_taker,
                    filled_at,
                    client_order_id,
                };

                let session_orders =
                    match db::list_orders(&self.pool, session_id, None).await {
                        Ok(orders) => orders,
                        Err(e) => {
                            error!(error = %e, "failed to list orders for fill import");
                            return;
                        }
                    };

                match fill_processor::import_fills(
                    &self.pool,
                    session_id,
                    &[fill],
                    &session_orders,
                    "ws_event",
                )
                .await
                {
                    Ok(import) => {
                        result.fills_recorded += import.recorded;
                        if import.recorded > 0 {
                            self.metrics
                                .reconciliation_fills_discovered
                                .inc_by(import.recorded);
                        }
                        if import.external_imported > 0 {
                            self.metrics
                                .fills_external_imported
                                .inc_by(import.external_imported);
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "fill import from WS failed");
                    }
                }
            }

            ExchangeEvent::OrderUpdate {
                exchange_order_id,
                client_order_id: _,
                ticker,
                status,
                filled_quantity,
                remaining_quantity: _,
                close_cancel_count,
            } => {
                // Look up the order by exchange_order_id (no session filter —
                // each harman instance has its own DB)
                match db::find_order_by_exchange_id(
                    &self.pool,
                    &exchange_order_id,
                )
                .await
                {
                    Ok(Some(order)) => {
                        self.audit.ws_event(
                            Some(order.session_id),
                            Some(order.id),
                            "order_update",
                            Some(serde_json::json!({
                                "exchange_order_id": exchange_order_id,
                                "ticker": ticker,
                                "status": format!("{:?}", status),
                                "filled_quantity": filled_quantity.to_string(),
                            })),
                            None,
                        );

                        // Handle unsolicited cancel
                        if status == OrderState::Cancelled {
                            let cancel_reason =
                                if order.state == OrderState::PendingCancel {
                                    CancelReason::UserRequested
                                } else if close_cancel_count.unwrap_or(0) > 0 {
                                    CancelReason::Expired
                                } else {
                                    CancelReason::ExchangeCancel
                                };

                            info!(
                                order_id = order.id,
                                exchange_order_id = %exchange_order_id,
                                cancel_reason = ?cancel_reason,
                                "WS: order cancelled"
                            );

                            if let Err(e) = db::update_order_state(
                                &self.pool,
                                order.id,
                                order.session_id,
                                OrderState::Cancelled,
                                Some(&exchange_order_id),
                                Some(&cancel_reason),
                                "ws_event",
                            )
                            .await
                            {
                                error!(error = %e, "failed to update cancelled state from WS");
                            } else {
                                result.orders_updated += 1;
                                // Cascade cancel to staged siblings in the same group
                                if order.group_id.is_some() {
                                    if let Err(e) = db::cancel_staged_group_siblings(
                                        &self.pool, order.id, order.session_id,
                                    ).await {
                                        error!(error = %e, "failed to cancel staged group siblings");
                                    }
                                }
                            }
                        } else if status == OrderState::Filled
                            && order.state != OrderState::Filled
                        {
                            // Order fully filled
                            info!(
                                order_id = order.id,
                                "WS: order fully filled"
                            );
                            if let Err(e) = db::update_order_state(
                                &self.pool,
                                order.id,
                                order.session_id,
                                OrderState::Filled,
                                Some(&exchange_order_id),
                                None,
                                "ws_event",
                            )
                            .await
                            {
                                error!(error = %e, "failed to update filled state from WS");
                            } else {
                                result.orders_updated += 1;
                                // Role-aware group handling on fill:
                                // Entry fill → activate staged TP/SL exits
                                // Exit fill → cancel sibling exit
                                if order.group_id.is_some() {
                                    match db::handle_group_on_fill(
                                        &self.pool, order.id, order.session_id,
                                    ).await {
                                        Ok(result) => {
                                            if result.activated_for_pump > 0 {
                                                info!(
                                                    order_id = order.id,
                                                    activated = result.activated_for_pump,
                                                    "bracket entry filled — activated exit legs, triggering pump"
                                                );
                                                self.pump_trigger.notify(order.session_id);
                                            }
                                            if let Some(ref pm) = self.price_monitor {
                                                for m in &result.monitoring_orders {
                                                    info!(
                                                        order_id = m.order_id,
                                                        ticker = %m.ticker,
                                                        trigger_price = %m.trigger_price,
                                                        "SL order entered monitoring — arming PriceMonitor"
                                                    );
                                                    pm.arm(crate::price_monitor::Trigger {
                                                        order_id: m.order_id,
                                                        session_id: m.session_id,
                                                        group_id: m.group_id,
                                                        ticker: m.ticker.clone(),
                                                        side: m.side,
                                                        action: m.action,
                                                        trigger_price: m.trigger_price,
                                                        submit_price: m.submit_price,
                                                        quantity: m.quantity,
                                                    });
                                                }
                                            } else if !result.monitoring_orders.is_empty() {
                                                warn!(
                                                    count = result.monitoring_orders.len(),
                                                    "orders entered Monitoring but PriceMonitor is not enabled"
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            error!(error = %e, "failed to handle group on fill");
                                        }
                                    }
                                }
                            }
                        } else if status == OrderState::Acknowledged
                            && order.state == OrderState::Submitted
                        {
                            // Submitted → Acknowledged (resting on exchange)
                            debug!(
                                order_id = order.id,
                                "WS: order now resting"
                            );
                            if let Err(e) = db::update_order_state(
                                &self.pool,
                                order.id,
                                order.session_id,
                                OrderState::Acknowledged,
                                Some(&exchange_order_id),
                                None,
                                "ws_event",
                            )
                            .await
                            {
                                error!(error = %e, "failed to update acknowledged state from WS");
                            } else {
                                result.orders_updated += 1;
                            }
                        }
                    }
                    Ok(None) => {
                        // Unknown order — WS user_orders doesn't carry side/action/price/quantity,
                        // so we can't construct a full ExchangeOrder for the importer.
                        self.audit.ws_event(
                            None,
                            None,
                            "order_update",
                            Some(serde_json::json!({
                                "exchange_order_id": exchange_order_id,
                                "ticker": ticker,
                                "status": format!("{:?}", status),
                                "filled_quantity": filled_quantity.to_string(),
                                "note": "external_order_not_imported",
                            })),
                            None,
                        );
                        if status == OrderState::Acknowledged || status == OrderState::Filled {
                            debug!(
                                exchange_order_id = %exchange_order_id,
                                ticker = %ticker,
                                status = %status,
                                "WS: detected external order (fill channel will import)"
                            );
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "failed to look up order by exchange_id");
                    }
                }
            }

            ExchangeEvent::PositionUpdate { .. } => {
                self.audit.ws_event(None, None, "position_update", None, None);
                debug!("WS: position update (informational)");
            }

            ExchangeEvent::MarketSettled {
                ref ticker,
                result: ref market_result,
                settled_time,
            } => {
                self.audit.ws_event(
                    None,
                    None,
                    "market_settled",
                    Some(serde_json::json!({
                        "ticker": ticker,
                        "market_result": format!("{:?}", market_result),
                        "settled_time": settled_time.to_rfc3339(),
                    })),
                    None,
                );

                // Check if any session holds an unsettled position for this ticker.
                // If so, fetch settlement data from REST (ticker-filtered) and record it.
                match db::sessions_with_unsettled_position(&self.pool, ticker).await {
                    Ok(session_ids) if !session_ids.is_empty() => {
                        info!(
                            ticker = %ticker,
                            market_result = ?market_result,
                            sessions = ?session_ids,
                            "WS: market settled — fetching settlement from REST"
                        );

                        // Retry with backoff: the REST API may not have the settlement
                        // immediately after the WS event fires (propagation delay).
                        let delays = [
                            std::time::Duration::from_secs(2),
                            std::time::Duration::from_secs(5),
                            std::time::Duration::from_secs(15),
                        ];
                        let mut recorded_target = false;

                        for (attempt, delay) in delays.iter().enumerate() {
                            tokio::time::sleep(*delay).await;

                            match self.exchange.get_settlements(None, Some(ticker)).await {
                                Ok(settlements) if !settlements.is_empty() => {
                                    for session_id in &session_ids {
                                        match settlement_recorder::record_settlements(
                                            &self.pool,
                                            *session_id,
                                            &settlements,
                                            "ws_settlement",
                                        ).await {
                                            Ok(count) => {
                                                if count > 0 {
                                                    info!(
                                                        session_id,
                                                        count,
                                                        ticker = %ticker,
                                                        attempt = attempt + 1,
                                                        "recorded settlement from WS event"
                                                    );
                                                }
                                                result.settlements_noted += count;
                                            }
                                            Err(e) => {
                                                error!(error = %e, ticker = %ticker, "failed to record settlement from WS event");
                                            }
                                        }
                                    }
                                    recorded_target = true;
                                    break;
                                }
                                Ok(_) => {
                                    warn!(
                                        ticker = %ticker,
                                        attempt = attempt + 1,
                                        max_attempts = delays.len(),
                                        "settlement not yet in REST response, will retry"
                                    );
                                }
                                Err(e) => {
                                    error!(error = %e, ticker = %ticker, attempt = attempt + 1, "failed to fetch settlement from REST");
                                }
                            }
                        }

                        if !recorded_target {
                            error!(
                                ticker = %ticker,
                                "settlement not found in REST after all retries — will be caught by recovery on next restart"
                            );
                        }
                    }
                    Ok(_) => {
                        debug!(ticker = %ticker, "WS: market settled (no local position)");
                    }
                    Err(e) => {
                        error!(error = %e, ticker = %ticker, "failed to check unsettled positions");
                    }
                }
            }

            ExchangeEvent::Connected => {
                self.audit.ws_event(None, None, "connected", None, None);
                info!("WS: connected");
                self.ws_connected.store(true, Ordering::Relaxed);
            }

            ExchangeEvent::Disconnected { reason } => {
                self.audit.ws_event(
                    None, None, "disconnected", None,
                    Some(serde_json::json!({"reason": reason})),
                );
                error!(reason = %reason, "WS: disconnected — crashing pod for K8s restart + recovery");
                // Flush the audit event before exit (give writer 500ms)
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                std::process::exit(1);
            }
        }
    }
}
