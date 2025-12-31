use ssmd_cache::config::Config;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env()?;
    tracing::info!(
        redis_url = %config.redis_url,
        nats_url = %config.nats_url,
        "Starting ssmd-cache"
    );

    // TODO: Implement cache warming and CDC consumption
    Ok(())
}
