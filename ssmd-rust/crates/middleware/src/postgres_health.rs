//! Postgres health check — spawns a background task that queries Postgres every 30s
//! and crashes the process if Postgres is unreachable.
//!
//! Gated behind the `postgres-health` feature flag.

use std::time::Duration;

/// Spawn a background tokio task that runs `SELECT 1` on Postgres every 30 seconds.
/// If the query fails, logs an error and exits the process with code 1.
/// K8s will restart the pod, forcing fresh connections and state reload.
pub fn spawn_postgres_health_check(pool: deadpool_postgres::Pool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.tick().await; // skip first tick — just connected
        loop {
            interval.tick().await;
            match pool.get().await {
                Ok(client) => {
                    if let Err(e) = client.execute("SELECT 1", &[]).await {
                        tracing::error!(error = %e, "Postgres health check query failed — exiting");
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Postgres health check pool error — exiting");
                    std::process::exit(1);
                }
            }
        }
    });
}
