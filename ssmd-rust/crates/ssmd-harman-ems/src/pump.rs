use serde::Serialize;
use std::sync::atomic::Ordering;
use tracing::{debug, error, info, warn};

use rust_decimal::Decimal;

use harman::db;
use harman::error::ExchangeError;
use harman::state::OrderState;
use harman::types::{AmendRequest, CancelReason};

use crate::Ems;

#[derive(Debug, Serialize)]
pub struct PumpResult {
    pub processed: u64,
    pub submitted: u64,
    pub rejected: u64,
    pub cancelled: u64,
    pub amended: u64,
    pub decreased: u64,
    pub requeued: u64,
    pub errors: Vec<String>,
}

/// Drain all pending queue items, submit/cancel to exchange, return results.
///
/// Processes items until the queue is empty or a rate limit is hit.
/// Called explicitly via `POST /v1/admin/pump` -- no background polling.
pub async fn pump(ems: &Ems, session_id: i64) -> PumpResult {
    let mut result = PumpResult {
        processed: 0,
        submitted: 0,
        rejected: 0,
        cancelled: 0,
        amended: 0,
        decreased: 0,
        requeued: 0,
        errors: vec![],
    };

    loop {
        if ems.shutting_down.load(Ordering::Relaxed) {
            result.errors.push("shutting down".into());
            break;
        }

        match db::dequeue_order(&ems.pool, session_id).await {
            Ok(Some(item)) => {
                ems.metrics.orders_dequeued.inc();
                result.processed += 1;
                debug!(
                    queue_id = item.queue_id,
                    order_id = item.order_id,
                    action = %item.action,
                    "dequeued order"
                );

                match item.action.as_str() {
                    "submit" => {
                        let outcome = handle_submit(ems, session_id, &item).await;
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
                        let outcome = handle_cancel(ems, session_id, &item).await;
                        match outcome {
                            CancelOutcome::Cancelled | CancelOutcome::NotFound => {
                                result.cancelled += 1;
                            }
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
                    "amend" => {
                        let outcome = handle_amend(ems, session_id, &item).await;
                        match outcome {
                            AmendOutcome::Amended => result.amended += 1,
                            AmendOutcome::Requeued(reason) => {
                                result.requeued += 1;
                                result.errors.push(reason);
                            }
                            AmendOutcome::RateLimited => {
                                result.requeued += 1;
                                result
                                    .errors
                                    .push("rate limited on amend, stopping early".into());
                                break;
                            }
                        }
                    }
                    "decrease" => {
                        let outcome = handle_decrease(ems, session_id, &item).await;
                        match outcome {
                            DecreaseOutcome::Decreased => result.decreased += 1,
                            DecreaseOutcome::Requeued(reason) => {
                                result.requeued += 1;
                                result.errors.push(reason);
                            }
                            DecreaseOutcome::RateLimited => {
                                result.requeued += 1;
                                result
                                    .errors
                                    .push("rate limited on decrease, stopping early".into());
                                break;
                            }
                        }
                    }
                    other => {
                        warn!(action = other, "unknown queue action, removing");
                        let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
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
        amended = result.amended,
        decreased = result.decreased,
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

async fn handle_submit(ems: &Ems, session_id: i64, item: &db::QueueItem) -> SubmitOutcome {
    match ems
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
            ems.metrics.orders_submitted.inc();

            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
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

            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            SubmitOutcome::Submitted
        }
        Err(ExchangeError::Rejected { reason }) => {
            warn!(
                order_id = item.order_id,
                reason = %reason,
                "order rejected by exchange"
            );
            ems.metrics.orders_rejected.inc();

            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
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

            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            SubmitOutcome::Rejected
        }
        Err(ExchangeError::RateLimited { retry_after_ms: _ }) => {
            warn!(order_id = item.order_id, "rate limited, requeueing");

            if let Err(e) = db::requeue_item(&ems.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue");
            }

            SubmitOutcome::RateLimited
        }
        Err(ExchangeError::Timeout { .. }) => {
            warn!(
                order_id = item.order_id,
                "exchange timeout, leaving as submitted for reconciliation"
            );
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            SubmitOutcome::Timeout
        }
        Err(e) => {
            error!(
                error = %e,
                order_id = item.order_id,
                "exchange error, requeueing"
            );

            if let Err(e) = db::requeue_item(&ems.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue");
            }

            SubmitOutcome::Requeued(format!("order {}: {}", item.order_id, e))
        }
    }
}

enum CancelOutcome {
    Cancelled,
    NotFound,
    Requeued(String),
    RateLimited,
}

async fn handle_cancel(ems: &Ems, session_id: i64, item: &db::QueueItem) -> CancelOutcome {
    let exchange_order_id = match &item.order.exchange_order_id {
        Some(id) => id.clone(),
        None => {
            info!(
                order_id = item.order_id,
                "cancel requested but never sent to exchange, cancelling locally"
            );
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
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
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            return CancelOutcome::Cancelled;
        }
    };

    match ems.exchange.cancel_order(&exchange_order_id).await {
        Ok(()) => {
            info!(order_id = item.order_id, "cancel confirmed");
            ems.metrics.orders_cancelled.inc();

            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
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

            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            CancelOutcome::Cancelled
        }
        Err(ExchangeError::NotFound(_)) => {
            info!(
                order_id = item.order_id,
                "cancel target not found on exchange, marking cancelled"
            );
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
                OrderState::Cancelled,
                None,
                None,
                Some(&CancelReason::UserRequested),
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to update cancelled state for not-found order");
            }
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            CancelOutcome::NotFound
        }
        Err(ExchangeError::RateLimited { retry_after_ms: _ }) => {
            if let Err(e) = db::requeue_item(&ems.pool, item.queue_id).await {
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
            if let Err(e) = db::requeue_item(&ems.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue cancel");
            }
            CancelOutcome::Requeued(format!("cancel order {}: {}", item.order_id, e))
        }
    }
}

enum AmendOutcome {
    Amended,
    Requeued(String),
    RateLimited,
}

async fn handle_amend(ems: &Ems, session_id: i64, item: &db::QueueItem) -> AmendOutcome {
    let exchange_order_id = match &item.order.exchange_order_id {
        Some(id) => id.clone(),
        None => {
            error!(
                order_id = item.order_id,
                "amend requested but no exchange_order_id, reverting state"
            );
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
                OrderState::Acknowledged,
                None,
                None,
                None,
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to revert amend state");
            }
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            return AmendOutcome::Requeued(format!(
                "order {} has no exchange_order_id for amend",
                item.order_id
            ));
        }
    };

    // Read amend params from metadata
    let metadata = match &item.metadata {
        Some(m) => m,
        None => {
            error!(order_id = item.order_id, "amend queue item missing metadata");
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
                OrderState::Acknowledged,
                None,
                None,
                None,
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to revert amend state");
            }
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            return AmendOutcome::Requeued(format!(
                "order {} amend missing metadata",
                item.order_id
            ));
        }
    };

    let new_price_dollars: Option<Decimal> = metadata
        .get("new_price_dollars")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok());
    let new_quantity: Option<Decimal> = metadata
        .get("new_quantity")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok());

    // Fill in missing values from current order -- Kalshi requires both price and quantity
    let request = AmendRequest {
        exchange_order_id: exchange_order_id.clone(),
        ticker: item.order.ticker.clone(),
        side: item.order.side,
        action: item.order.action,
        new_price_dollars: Some(new_price_dollars.unwrap_or(item.order.price_dollars)),
        new_quantity: Some(new_quantity.unwrap_or(item.order.quantity)),
    };

    match ems.exchange.amend_order(&request).await {
        Ok(result) => {
            info!(
                order_id = item.order_id,
                new_exchange_order_id = %result.exchange_order_id,
                new_price = %result.new_price_dollars,
                new_quantity = %result.new_quantity,
                "order amended on exchange"
            );

            // Update order with new values from exchange
            let client = match ems.pool.get().await {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "pool error updating amended order");
                    let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
                    return AmendOutcome::Amended;
                }
            };

            if let Err(e) = client
                .execute(
                    "UPDATE prediction_orders SET state = 'acknowledged', \
                     exchange_order_id = $1, price_dollars = $2, quantity = $3 \
                     WHERE id = $4 AND session_id = $5",
                    &[
                        &result.exchange_order_id,
                        &result.new_price_dollars,
                        &result.new_quantity,
                        &item.order_id,
                        &session_id,
                    ],
                )
                .await
            {
                error!(error = %e, "failed to update amended order");
            }

            // Audit log
            if let Err(e) = client
                .execute(
                    "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
                     VALUES ($1, 'pending_amend', 'acknowledged', 'amend_confirm', 'pump')",
                    &[&item.order_id],
                )
                .await
            {
                error!(error = %e, "failed to insert amend audit");
            }

            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            AmendOutcome::Amended
        }
        Err(ExchangeError::NotFound(_)) => {
            warn!(
                order_id = item.order_id,
                "amend target not found on exchange, marking cancelled"
            );
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
                OrderState::Cancelled,
                None,
                None,
                Some(&CancelReason::ExchangeCancel),
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to cancel not-found amend order");
            }
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            AmendOutcome::Requeued(format!(
                "amend order {} not found on exchange, cancelled",
                item.order_id
            ))
        }
        Err(ExchangeError::RateLimited { retry_after_ms: _ }) => {
            warn!(order_id = item.order_id, "rate limited on amend, requeueing");
            if let Err(e) = db::requeue_item(&ems.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue amend");
            }
            AmendOutcome::RateLimited
        }
        Err(e) => {
            error!(
                error = %e,
                order_id = item.order_id,
                "amend exchange error, reverting state"
            );
            // Revert to acknowledged on failure
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
                OrderState::Acknowledged,
                None,
                None,
                None,
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to revert amend state");
            }
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            AmendOutcome::Requeued(format!("amend order {}: {}", item.order_id, e))
        }
    }
}

enum DecreaseOutcome {
    Decreased,
    Requeued(String),
    RateLimited,
}

async fn handle_decrease(ems: &Ems, session_id: i64, item: &db::QueueItem) -> DecreaseOutcome {
    let exchange_order_id = match &item.order.exchange_order_id {
        Some(id) => id.clone(),
        None => {
            error!(
                order_id = item.order_id,
                "decrease requested but no exchange_order_id, reverting state"
            );
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
                OrderState::Acknowledged,
                None,
                None,
                None,
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to revert decrease state");
            }
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            return DecreaseOutcome::Requeued(format!(
                "order {} has no exchange_order_id for decrease",
                item.order_id
            ));
        }
    };

    // Read reduce_by from metadata
    let reduce_by: Decimal = match &item.metadata {
        Some(m) => match m.get("reduce_by").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()) {
            Some(d) => d,
            None => {
                error!(order_id = item.order_id, "decrease metadata missing reduce_by");
                if let Err(e) = db::update_order_state(
                    &ems.pool,
                    item.order_id,
                    session_id,
                    OrderState::Acknowledged,
                    None,
                    None,
                    None,
                    "pump",
                )
                .await
                {
                    error!(error = %e, "failed to revert decrease state");
                }
                let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
                return DecreaseOutcome::Requeued(format!(
                    "order {} decrease missing reduce_by",
                    item.order_id
                ));
            }
        },
        None => {
            error!(order_id = item.order_id, "decrease queue item missing metadata");
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
                OrderState::Acknowledged,
                None,
                None,
                None,
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to revert decrease state");
            }
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            return DecreaseOutcome::Requeued(format!(
                "order {} decrease missing metadata",
                item.order_id
            ));
        }
    };

    match ems
        .exchange
        .decrease_order(&exchange_order_id, reduce_by)
        .await
    {
        Ok(()) => {
            info!(
                order_id = item.order_id,
                reduce_by = %reduce_by,
                "order decreased on exchange"
            );

            // Update quantity in DB
            let client = match ems.pool.get().await {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "pool error updating decreased order");
                    let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
                    return DecreaseOutcome::Decreased;
                }
            };

            if let Err(e) = client
                .execute(
                    "UPDATE prediction_orders SET state = 'acknowledged', \
                     quantity = quantity - $1 \
                     WHERE id = $2 AND session_id = $3",
                    &[&reduce_by, &item.order_id, &session_id],
                )
                .await
            {
                error!(error = %e, "failed to update decreased order");
            }

            // Audit log
            if let Err(e) = client
                .execute(
                    "INSERT INTO audit_log (order_id, from_state, to_state, event, actor) \
                     VALUES ($1, 'pending_decrease', 'acknowledged', 'decrease_confirm', 'pump')",
                    &[&item.order_id],
                )
                .await
            {
                error!(error = %e, "failed to insert decrease audit");
            }

            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            DecreaseOutcome::Decreased
        }
        Err(ExchangeError::NotFound(_)) => {
            warn!(
                order_id = item.order_id,
                "decrease target not found on exchange, marking cancelled"
            );
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
                OrderState::Cancelled,
                None,
                None,
                Some(&CancelReason::ExchangeCancel),
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to cancel not-found decrease order");
            }
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            DecreaseOutcome::Requeued(format!(
                "decrease order {} not found on exchange, cancelled",
                item.order_id
            ))
        }
        Err(ExchangeError::RateLimited { retry_after_ms: _ }) => {
            warn!(order_id = item.order_id, "rate limited on decrease, requeueing");
            if let Err(e) = db::requeue_item(&ems.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue decrease");
            }
            DecreaseOutcome::RateLimited
        }
        Err(e) => {
            error!(
                error = %e,
                order_id = item.order_id,
                "decrease exchange error, reverting state"
            );
            if let Err(e) = db::update_order_state(
                &ems.pool,
                item.order_id,
                session_id,
                OrderState::Acknowledged,
                None,
                None,
                None,
                "pump",
            )
            .await
            {
                error!(error = %e, "failed to revert decrease state");
            }
            let _ = db::remove_queue_item(&ems.pool, item.queue_id).await;
            DecreaseOutcome::Requeued(format!("decrease order {}: {}", item.order_id, e))
        }
    }
}
