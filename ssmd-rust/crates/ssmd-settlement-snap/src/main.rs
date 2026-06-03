//! ssmd-settlement-snap: captures final snap + outcome of each settled 15-minute
//! crypto market to GCS for model training.
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    info!("ssmd-settlement-snap starting");
    Ok(())
}
