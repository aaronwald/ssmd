use std::sync::Arc;
use tracing::info;

use crate::AppState;

/// Wait for shutdown signal (SIGTERM or ctrl-c) and initiate graceful shutdown.
///
/// Signal listening stays in the binary. Shutdown execution is delegated to EMS.
pub async fn wait_for_shutdown(state: Arc<AppState>) {
    shutdown_signal().await;
    info!("shutdown signal received");
    // Stop background tasks first (auto-pump, auto-reconcile)
    state.runner.shutdown();
    // Then EMS shutdown (mass cancel + drain)
    state.ems.shutdown().await;
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
