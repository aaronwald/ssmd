use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::AppState;

/// Wait for shutdown signal (SIGTERM or ctrl-c) and initiate graceful shutdown.
///
/// Sequence:
/// 1. Set shutting_down flag (API returns 503, pump returns early)
/// 2. Mass cancel all open orders on exchange
/// 3. Drain remaining queue items across ALL sessions (reject them without transitioning to Submitted)
/// 4. Exit
pub async fn wait_for_shutdown(state: Arc<AppState>) {
    shutdown_signal().await;

    info!("shutdown signal received");
    state.shutting_down.store(true, Ordering::Relaxed);

    // Mass cancel on exchange
    match state.exchange.cancel_all_orders().await {
        Ok(count) => info!(count, "mass cancel completed"),
        Err(e) => error!(error = %e, "mass cancel failed during shutdown"),
    }

    // Drain queue for ALL sessions — during pod shutdown, reject everything
    match harman::db::drain_queue_for_shutdown_all(&state.pool).await {
        Ok(count) => {
            if count > 0 {
                warn!(count, "drained queue items during shutdown");
            }
        }
        Err(e) => error!(error = %e, "drain queue failed"),
    }

    info!("shutdown complete");
}

/// Listen for SIGTERM (Kubernetes pod termination) or ctrl-c.
#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to listen for SIGTERM");
    let ctrl_c = tokio::signal::ctrl_c();

    tokio::select! {
        _ = sigterm.recv() => info!("SIGTERM received"),
        _ = ctrl_c => info!("ctrl-c received"),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
}
