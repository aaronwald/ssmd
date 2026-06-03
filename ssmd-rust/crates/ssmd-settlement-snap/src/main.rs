//! ssmd-settlement-snap: captures final snap + outcome of each settled 15-minute
//! crypto market to GCS for model training.
// Foundation modules (Tasks 2-7) are built and tested independently before the
// consumer run-loop (Task 8) wires them into the entrypoint. Until then their
// public items are exercised only by unit tests, so suppress dead-code warnings.
#![allow(dead_code)]
use tracing::info;

mod symbology;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    info!("ssmd-settlement-snap starting");
    Ok(())
}
