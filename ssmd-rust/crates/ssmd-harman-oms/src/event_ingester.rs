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

use harman::db;
use harman::exchange::ExchangeEvent;
use harman::fill_processor;
use harman::state::OrderState;
use harman::types::{CancelReason, ExchangeFill};

use crate::OmsMetrics;

/// Ingests WS events and writes to DB via shared processors.
pub struct EventIngester {
    pool: Pool,
    session_id: i64,
    metrics: Arc<OmsMetrics>,
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
    pub fn new(pool: Pool, session_id: i64, metrics: Arc<OmsMetrics>) -> Self {
        Self {
            pool,
            session_id,
            metrics,
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
                    match db::list_orders(&self.pool, self.session_id, None).await {
                        Ok(orders) => orders,
                        Err(e) => {
                            error!(error = %e, "failed to list orders for fill import");
                            return;
                        }
                    };

                match fill_processor::import_fills(
                    &self.pool,
                    self.session_id,
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
                // Look up the order by exchange_order_id
                match db::find_order_by_exchange_id(
                    &self.pool,
                    self.session_id,
                    &exchange_order_id,
                )
                .await
                {
                    Ok(Some(order)) => {
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
                                self.session_id,
                                OrderState::Cancelled,
                                Some(&exchange_order_id),
                                Some(filled_quantity),
                                Some(&cancel_reason),
                                "ws_event",
                            )
                            .await
                            {
                                error!(error = %e, "failed to update cancelled state from WS");
                            } else {
                                result.orders_updated += 1;
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
                                self.session_id,
                                OrderState::Filled,
                                Some(&exchange_order_id),
                                Some(filled_quantity),
                                None,
                                "ws_event",
                            )
                            .await
                            {
                                error!(error = %e, "failed to update filled state from WS");
                            } else {
                                result.orders_updated += 1;
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
                                self.session_id,
                                OrderState::Acknowledged,
                                Some(&exchange_order_id),
                                Some(filled_quantity),
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
                        // Log it; the fill channel or REST reconciliation will handle import.
                        if status == OrderState::Acknowledged || status == OrderState::Filled {
                            debug!(
                                exchange_order_id = %exchange_order_id,
                                ticker = %ticker,
                                status = %status,
                                "WS: detected external order (fill or REST reconciliation will import)"
                            );
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "failed to look up order by exchange_id");
                    }
                }
            }

            ExchangeEvent::PositionUpdate { .. } => {
                // Position updates are informational for now.
                // Phase 5 will add in-memory position cache.
                debug!("WS: position update (informational)");
            }

            ExchangeEvent::MarketSettled {
                ticker,
                result: market_result,
                settled_time,
            } => {
                // WS market_lifecycle_v2 only provides ticker/result/time — not the P&L
                // fields (event_ticker, yes_count, no_count, revenue_cents, etc.) needed
                // to record a full settlement. REST reconciliation handles that.
                //
                // This event is useful for knowing a market settled, which helps with
                // unsolicited cancel detection (orders auto-cancelled at settlement).
                info!(
                    ticker = %ticker,
                    market_result = ?market_result,
                    settled_time = %settled_time,
                    "WS: market settled (REST reconciliation will record P&L)"
                );
                result.settlements_noted += 1;
            }

            ExchangeEvent::Connected => {
                info!("WS: connected");
                self.ws_connected.store(true, Ordering::Relaxed);
            }

            ExchangeEvent::Disconnected { reason } => {
                warn!(reason = %reason, "WS: disconnected");
                self.ws_connected.store(false, Ordering::Relaxed);
            }
        }
    }
}
