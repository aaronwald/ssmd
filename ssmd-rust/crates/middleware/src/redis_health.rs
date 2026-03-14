//! Redis health check — spawns a background task that PINGs Redis every 30s
//! and crashes the process if Redis is unreachable.
//!
//! Gated behind the `redis-health` feature flag.

use std::time::Duration;

/// Spawn a background tokio task that PINGs Redis every 30 seconds.
/// If the PING fails, logs an error and exits the process with code 1.
/// K8s will restart the pod, forcing a fresh connection and cache rebuild.
pub fn spawn_redis_health_check(conn: redis::aio::MultiplexedConnection) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.tick().await; // skip first tick — just connected
        loop {
            interval.tick().await;
            let mut c = conn.clone();
            if let Err(e) = redis::cmd("PING")
                .query_async::<String>(&mut c)
                .await
            {
                tracing::error!(error = %e, "Redis health check failed — exiting");
                std::process::exit(1);
            }
        }
    });
}
