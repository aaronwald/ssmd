//! ssmd-settlement-snap: captures final snap + outcome of each settled 15-minute
//! crypto market to GCS for model training.

// Foundation modules expose a small amount of API surface (e.g. lifecycle
// `open_ts`/`settled_ts`, `LastTickMap::len`) kept for completeness and used by
// tests, that the wired run loop does not read directly.
#![allow(dead_code)]

use tracing::info;

mod config;
mod consumer;
mod gcs;
mod lifecycle;
mod metrics;
mod reconcile;
mod record;
mod symbology;
mod ticker;

use config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ssmd_settlement_snap=info".into()),
        )
        .init();
    info!("ssmd-settlement-snap starting");

    // Fail loud on missing/invalid config at startup.
    let config = Config::from_env()?;

    // Run the consumers. Any fatal condition returns Err; we exit non-zero so
    // K8s restarts the pod (crash-cascade policy — no limping).
    if let Err(e) = consumer::run(config).await {
        tracing::error!(error = %e, "settlement-snap exiting (fatal)");
        std::process::exit(1);
    }

    Ok(())
}
