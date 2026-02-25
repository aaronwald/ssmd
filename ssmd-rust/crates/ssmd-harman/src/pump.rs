use serde::Serialize;
use std::sync::atomic::Ordering;
use tracing::{debug, error, info, warn};

use harman::db;
use harman::error::ExchangeError;
use harman::state::OrderState;
use harman::types::CancelReason;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct PumpResult {
    pub processed: u64,
    pub submitted: u64,
    pub rejected: u64,
    pub cancelled: u64,
    pub requeued: u64,
    pub errors: Vec<String>,
}

/// Drain all pending queue items, submit/cancel to exchange, return results.
///
/// Processes items until the queue is empty or a rate limit is hit.
/// Called explicitly via `POST /v1/admin/pump` — no background polling.
pub async fn pump(state: &AppState) -> PumpResult {
    let mut result = PumpResult {
        processed: 0,
        submitted: 0,
        rejected: 0,
        cancelled: 0,
        requeued: 0,
        errors: vec![],
    };

    loop {
        if state.shutting_down.load(Ordering::Relaxed) {
            result.errors.push("shutting down".into());
            break;
        }

        match db::dequeue_order(&state.pool, state.session_id).await {
            Ok(Some(item)) => {
                state.metrics.orders_dequeued.inc();
                result.processed += 1;
                debug!(
                    queue_id = item.queue_id,
                    order_id = item.order_id,
                    action = %item.action,
                    "dequeued order"
                );

                match item.action.as_str() {
                    "submit" => {
                        let outcome = handle_submit(state, &item).await;
                        match outcome {
                            SubmitOutcome::Submitted => result.submitted += 1,
                            SubmitOutcome::Rejected => result.rejected += 1,
                            SubmitOutcome::Timeout => {
                                result.errors.push(format!(
                                    "order {} timed out, left for reconciliation",
                                    item.order_id
                                ));
                            }
                            SubmitOutcome::Requeued(reason) => {
                                result.requeued += 1;
                                result.errors.push(reason);
                            }
                            SubmitOutcome::RateLimited => {
                                result.requeued += 1;
                                result
                                    .errors
                                    .push("rate limited, stopping early".into());
                                break;
                            }
                        }
                    }
                    "cancel" => {
                        let outcome = handle_cancel(state, &item).await;
                        match outcome {
                            CancelOutcome::Cancelled | CancelOutcome::NotFound => {
                                result.cancelled += 1;
                            }
                            CancelOutcome::Skipped => {}
                            CancelOutcome::Requeued(reason) => {
                                result.requeued += 1;
                                result.errors.push(reason);
                            }
                            CancelOutcome::RateLimited => {
                                result.requeued += 1;
                                result
                                    .errors
                                    .push("rate limited on cancel, stopping early".into());
                                break;
                            }
                        }
                    }
                    other => {
                        warn!(action = other, "unknown queue action, removing");
                        let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                let msg = format!("dequeue failed: {}", e);
                error!(error = %e, "dequeue failed");
                result.errors.push(msg);
                break;
            }
        }
    }

    info!(
        processed = result.processed,
        submitted = result.submitted,
        rejected = result.rejected,
        cancelled = result.cancelled,
        requeued = result.requeued,
        errors = result.errors.len(),
        "pump complete"
    );

    result
}

enum SubmitOutcome {
    Submitted,
    Rejected,
    Timeout,
    Requeued(String),
    RateLimited,
}

async fn handle_submit(state: &AppState, item: &db::QueueItem) -> SubmitOutcome {
    match state
        .exchange
        .submit_order(&harman::types::OrderRequest {
            client_order_id: item.order.client_order_id,
            ticker: item.order.ticker.clone(),
            side: item.order.side,
            action: item.order.action,
            quantity: item.order.quantity,
            price_dollars: item.order.price_dollars,
            time_in_force: item.order.time_in_force,
        })
        .await
    {
        Ok(exchange_order_id) => {
            info!(
                order_id = item.order_id,
                exchange_order_id = %exchange_order_id,
                "order acknowledged by exchange"
            );
            state.metrics.orders_submitted.inc();

            if let Err(e) = db::update_order_state(
                &state.pool,
                item.order_id,
                state.session_id,
                OrderState::Acknowledged,
                Some(&exchange_order_id),
                None,
                None,
                "pump",
            )
            .await
            {
                error!(error = %e, order_id = item.order_id, "failed to update order state");
            }

            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
            SubmitOutcome::Submitted
        }
        Err(ExchangeError::Rejected { reason }) => {
            warn!(
                order_id = item.order_id,
                reason = %reason,
                "order rejected by exchange"
            );
            state.metrics.orders_rejected.inc();

            if let Err(e) = db::update_order_state(
                &state.pool,
                item.order_id,
                state.session_id,
                OrderState::Rejected,
                None,
                None,
                None,
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to update rejected state");
            }

            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
            SubmitOutcome::Rejected
        }
        Err(ExchangeError::RateLimited { retry_after_ms: _ }) => {
            warn!(order_id = item.order_id, "rate limited, requeueing");

            if let Err(e) = db::requeue_item(&state.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue");
            }

            SubmitOutcome::RateLimited
        }
        Err(ExchangeError::Timeout { .. }) => {
            warn!(
                order_id = item.order_id,
                "exchange timeout, leaving as submitted for reconciliation"
            );
            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
            SubmitOutcome::Timeout
        }
        Err(e) => {
            error!(
                error = %e,
                order_id = item.order_id,
                "exchange error, requeueing"
            );

            if let Err(e) = db::requeue_item(&state.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue");
            }

            SubmitOutcome::Requeued(format!("order {}: {}", item.order_id, e))
        }
    }
}

enum CancelOutcome {
    Cancelled,
    NotFound,
    Skipped,
    Requeued(String),
    RateLimited,
}

async fn handle_cancel(state: &AppState, item: &db::QueueItem) -> CancelOutcome {
    let exchange_order_id = match &item.order.exchange_order_id {
        Some(id) => id.clone(),
        None => {
            info!(
                order_id = item.order_id,
                "cancel requested but never sent to exchange, cancelling locally"
            );
            if let Err(e) = db::update_order_state(
                &state.pool,
                item.order_id,
                state.session_id,
                OrderState::Cancelled,
                None,
                None,
                Some(&CancelReason::UserRequested),
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to update cancelled state");
            }
            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
            return CancelOutcome::Cancelled;
        }
    };

    match state.exchange.cancel_order(&exchange_order_id).await {
        Ok(()) => {
            info!(order_id = item.order_id, "cancel confirmed");
            state.metrics.orders_cancelled.inc();

            if let Err(e) = db::update_order_state(
                &state.pool,
                item.order_id,
                state.session_id,
                OrderState::Cancelled,
                None,
                None,
                Some(&CancelReason::UserRequested),
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to update cancelled state");
            }

            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
            CancelOutcome::Cancelled
        }
        Err(ExchangeError::NotFound(_)) => {
            warn!(
                order_id = item.order_id,
                "cancel target not found on exchange, reconciliation will resolve"
            );
            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
            CancelOutcome::NotFound
        }
        Err(ExchangeError::RateLimited { retry_after_ms: _ }) => {
            if let Err(e) = db::requeue_item(&state.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue cancel");
            }
            CancelOutcome::RateLimited
        }
        Err(e) => {
            error!(
                error = %e,
                order_id = item.order_id,
                "cancel exchange error, requeueing"
            );
            if let Err(e) = db::requeue_item(&state.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue cancel");
            }
            CancelOutcome::Requeued(format!("cancel order {}: {}", item.order_id, e))
        }
    }
}
