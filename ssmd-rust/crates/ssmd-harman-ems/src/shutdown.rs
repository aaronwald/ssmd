use std::sync::atomic::Ordering;
use tracing::{error, info, warn};

use crate::Ems;

/// Execute shutdown sequence: set flag, mass cancel, drain queue.
///
/// Does NOT listen for signals -- that stays in the binary.
/// This just executes the shutdown actions.
pub async fn shutdown(ems: &Ems) {
    info!("EMS shutdown initiated");
    ems.shutting_down.store(true, Ordering::Relaxed);

    // Mass cancel on exchange
    match ems.exchange.cancel_all_orders().await {
        Ok(count) => info!(count, "mass cancel completed"),
        Err(e) => error!(error = %e, "mass cancel failed during shutdown"),
    }

    // Drain queue for ALL sessions
    match harman::db::drain_queue_for_shutdown_all(&ems.pool).await {
        Ok(count) => {
            if count > 0 {
                warn!(count, "drained queue items during shutdown");
            }
        }
        Err(e) => error!(error = %e, "drain queue failed"),
    }

    info!("EMS shutdown complete");
}
