use axum::{routing::get, Router, response::IntoResponse};
use prometheus::{Registry, TextEncoder, Encoder};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_cache::{
    config::Config,
    cache::RedisCache,
    warmer::CacheWarmer,
    consumer::CdcConsumer,
    metrics::CacheMetrics,
};

/// Prometheus metrics endpoint
async fn metrics_handler(
    axum::extract::State(registry): axum::extract::State<Registry>,
) -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = registry.gather();
    let mut buf = Vec::new();
    encoder.encode(&metric_families, &mut buf).unwrap_or_default();
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        buf,
    )
}

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

    // Set up Prometheus metrics
    let registry = Registry::new();
    let cache_metrics = CacheMetrics::new(&registry)
        .expect("Failed to register cache metrics");

    // Spawn metrics HTTP server on port 9090
    let metrics_registry = registry.clone();
    tokio::spawn(async move {
        let app = Router::new()
            .route("/metrics", get(metrics_handler))
            .route("/health", get(|| async { "ok" }))
            .with_state(metrics_registry);

        let listener = tokio::net::TcpListener::bind("0.0.0.0:9090").await.unwrap();
        tracing::info!("Metrics server listening on 0.0.0.0:9090");
        axum::serve(listener, app).await.unwrap();
    });

    // Create shared Postgres pool (max_size=4: warmer, CDC lookup, lifecycle writes, health check)
    let pg_pool = {
        let mut cfg = deadpool_postgres::Config::new();
        cfg.url = Some(config.database_url.clone());
        cfg.pool = Some(deadpool_postgres::PoolConfig { max_size: 4, ..Default::default() });
        cfg.create_pool(
            Some(deadpool_postgres::Runtime::Tokio1),
            tokio_postgres::NoTls,
        ).expect("Failed to create Postgres pool")
    };

    // Connect to Redis
    let cache = RedisCache::new(&config.redis_url).await?;

    // Warm cache from PostgreSQL
    let warmer = CacheWarmer::new(pg_pool.clone());
    let snapshot_lsn = warmer.warm_all(&cache).await?;

    // Spawn periodic monitor index refresh (every 5 minutes)
    let refresh_cache = cache.clone();
    let refresh_warmer = CacheWarmer::new(pg_pool.clone());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        interval.tick().await; // skip first tick — warm_all already ran
        loop {
            interval.tick().await;
            match refresh_warmer.warm_monitor_indexes(&refresh_cache).await {
                Ok(keys) => tracing::info!(keys, "Periodic monitor index refresh"),
                Err(e) => {
                    tracing::error!(error = %e, "Monitor index refresh failed — exiting");
                    std::process::exit(1);
                }
            }
        }
    });

    // Spawn Redis health check (every 30s — crash if Redis is unreachable)
    ssmd_middleware::redis_health::spawn_redis_health_check(cache.connection());

    // Spawn Postgres health check (every 30s — crash if Postgres is unreachable)
    ssmd_middleware::postgres_health::spawn_postgres_health_check(pg_pool.clone());

    // Start consuming CDC events
    let mut consumer = CdcConsumer::new(
        &config.nats_url,
        &config.stream_name,
        &config.consumer_name,
        snapshot_lsn,
        pg_pool.clone(),
        cache_metrics,
    ).await?;

    consumer.run(&cache).await?;

    Ok(())
}
