use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_cache::{
    config::Config,
    cache::RedisCache,
    warmer::CacheWarmer,
    consumer::CdcConsumer,
};

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
        stream = %config.stream_name,
        "Starting ssmd-cache"
    );

    // Connect to Redis
    let cache = RedisCache::new(&config.redis_url).await?;

    // Connect to PostgreSQL and warm cache
    let warmer = CacheWarmer::connect(&config.database_url).await?;
    let snapshot_lsn = warmer.warm_all(&cache).await?;

    // Log cache stats
    let series = cache.count("secmaster:series:*").await?;
    let markets = cache.count("secmaster:series:*:market:*").await?;
    let events = cache.count("secmaster:event:*").await?;
    let fees = cache.count("secmaster:fee:*").await?;
    tracing::info!(series, markets, events, fees, "Cache populated");

    // Start consuming CDC events
    let mut consumer = CdcConsumer::new(
        &config.nats_url,
        &config.stream_name,
        &config.consumer_name,
        snapshot_lsn,
        &config.database_url,
    ).await?;

    consumer.run(&cache).await?;

    Ok(())
}
