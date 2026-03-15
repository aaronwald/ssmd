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

    // Connect to Redis
    let cache = RedisCache::new(&config.redis_url).await?;

    // Connect to PostgreSQL and warm cache
    let warmer = CacheWarmer::connect(&config.database_url).await?;
    let snapshot_lsn = warmer.warm_all(&cache).await?;

    // Spawn periodic monitor index refresh (every 5 minutes)
    let refresh_cache = cache.clone();
    let refresh_db_url = config.database_url.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        interval.tick().await; // skip first tick — warm_all already ran
        loop {
            interval.tick().await;
            match CacheWarmer::connect(&refresh_db_url).await {
                Ok(warmer) => {
                    match warmer.warm_monitor_indexes(&refresh_cache).await {
                        Ok(keys) => tracing::info!(keys, "Periodic monitor index refresh"),
                        Err(e) => tracing::error!(error = %e, "Monitor index refresh failed"),
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "DB connect failed for periodic refresh — exiting");
                    std::process::exit(1);
                }
            }
        }
    });

    // Spawn Redis health check (every 30s — crash if Redis is unreachable)
    ssmd_middleware::redis_health::spawn_redis_health_check(cache.connection());

    // Spawn Postgres health check (every 30s — crash if Postgres is unreachable)
    {
        let mut pg_cfg = deadpool_postgres::Config::new();
        let pg_config: tokio_postgres::Config = config.database_url.parse()
            .expect("DATABASE_URL already validated");
        if let Some(host) = pg_config.get_hosts().first() {
            match host {
                tokio_postgres::config::Host::Tcp(h) => pg_cfg.host = Some(h.clone()),
                #[cfg(unix)]
                tokio_postgres::config::Host::Unix(p) => {
                    pg_cfg.host = Some(p.to_string_lossy().to_string())
                }
            }
        }
        if let Some(port) = pg_config.get_ports().first() {
            pg_cfg.port = Some(*port);
        }
        if let Some(user) = pg_config.get_user() {
            pg_cfg.user = Some(user.to_string());
        }
        if let Some(password) = pg_config.get_password() {
            pg_cfg.password = Some(String::from_utf8_lossy(password).to_string());
        }
        if let Some(dbname) = pg_config.get_dbname() {
            pg_cfg.dbname = Some(dbname.to_string());
        }
        pg_cfg.pool = Some(deadpool_postgres::PoolConfig { max_size: 1, ..Default::default() });
        let pg_pool = pg_cfg.create_pool(
            Some(deadpool_postgres::Runtime::Tokio1),
            tokio_postgres::NoTls,
        ).expect("Failed to create Postgres health check pool");
        ssmd_middleware::postgres_health::spawn_postgres_health_check(pg_pool);
    }

    // Start consuming CDC events
    let mut consumer = CdcConsumer::new(
        &config.nats_url,
        &config.stream_name,
        &config.consumer_name,
        snapshot_lsn,
        &config.database_url,
        cache_metrics,
    ).await?;

    consumer.run(&cache).await?;

    Ok(())
}
