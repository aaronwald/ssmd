use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use harman::db;
use harman::error::ExchangeError;
use harman::state::OrderState;
use harman::types::CancelReason;

use crate::AppState;

const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Run the sweeper loop.
///
/// Dequeues orders from the queue, submits them to the exchange,
/// and updates state based on the response.
pub async fn run(state: Arc<AppState>) {
    info!("sweeper started");

    loop {
        if state.shutting_down.load(Ordering::Relaxed) {
            info!("sweeper shutting down");
            break;
        }

        match db::dequeue_order(&state.pool).await {
            Ok(Some(item)) => {
                state.metrics.orders_dequeued.inc();
                debug!(
                    queue_id = item.queue_id,
                    order_id = item.order_id,
                    action = %item.action,
                    "dequeued order"
                );

                match item.action.as_str() {
                    "submit" => handle_submit(&state, &item).await,
                    "cancel" => handle_cancel(&state, &item).await,
                    other => {
                        warn!(action = other, "unknown queue action, removing");
                        let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
                    }
                }
            }
            Ok(None) => {
                // No items in queue, sleep
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            Err(e) => {
                error!(error = %e, "dequeue failed");
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    }
}

async fn handle_submit(state: &Arc<AppState>, item: &db::QueueItem) {
    match state.exchange.submit_order(&harman::types::OrderRequest {
        client_order_id: item.order.client_order_id,
        ticker: item.order.ticker.clone(),
        side: item.order.side,
        action: item.order.action,
        quantity: item.order.quantity,
        price_cents: item.order.price_cents,
        time_in_force: item.order.time_in_force,
    }).await {
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
                OrderState::Acknowledged,
                Some(&exchange_order_id),
                None,
                None,
            )
            .await
            {
                error!(error = %e, order_id = item.order_id, "failed to update order state");
            }

            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
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
                OrderState::Rejected,
                None,
                None,
                None,
            )
            .await
            {
                error!(error = %e, "failed to update rejected state");
            }

            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
        }
        Err(ExchangeError::RateLimited { retry_after_ms }) => {
            warn!(
                order_id = item.order_id,
                retry_after_ms,
                "rate limited, requeueing"
            );

            // Requeue for retry
            if let Err(e) = db::requeue_item(&state.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue");
            }

            // Sleep to respect rate limit
            tokio::time::sleep(Duration::from_millis(retry_after_ms)).await;
        }
        Err(ExchangeError::Timeout { .. }) => {
            warn!(
                order_id = item.order_id,
                "exchange timeout, leaving as submitted for reconciliation"
            );
            // Don't requeue - reconciliation will handle it
            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
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

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

async fn handle_cancel(state: &Arc<AppState>, item: &db::QueueItem) {
    let exchange_order_id = match &item.order.exchange_order_id {
        Some(id) => id.clone(),
        None => {
            warn!(
                order_id = item.order_id,
                "cancel requested but no exchange_order_id, skipping"
            );
            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
            return;
        }
    };

    match state.exchange.cancel_order(&exchange_order_id).await {
        Ok(()) => {
            info!(order_id = item.order_id, "cancel confirmed");
            state.metrics.orders_cancelled.inc();

            if let Err(e) = db::update_order_state(
                &state.pool,
                item.order_id,
                OrderState::Cancelled,
                None,
                None,
                Some(&CancelReason::UserRequested),
            )
            .await
            {
                error!(error = %e, "failed to update cancelled state");
            }

            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
        }
        Err(ExchangeError::NotFound(_)) => {
            // Order already gone - might have been filled
            warn!(
                order_id = item.order_id,
                "cancel target not found on exchange, reconciliation will resolve"
            );
            let _ = db::remove_queue_item(&state.pool, item.queue_id).await;
        }
        Err(ExchangeError::RateLimited { retry_after_ms }) => {
            if let Err(e) = db::requeue_item(&state.pool, item.queue_id).await {
                error!(error = %e, "failed to requeue cancel");
            }
            tokio::time::sleep(Duration::from_millis(retry_after_ms)).await;
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
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
