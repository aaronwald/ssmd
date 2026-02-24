mod config;
mod metrics;
mod snap;

use std::sync::Arc;

use axum::{extract::State, routing::get, Router};
use clap::Parser;
use tracing::info;

use config::{parse_stream, Config};
use metrics::Metrics;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ssmd_snap=info".into()),
        )
        .json()
        .init();

    let config = Config::parse();
    let streams: Vec<&str> = config.streams.split(',').map(|s| s.trim()).collect();

    info!(
        nats_url = %config.nats_url,
        redis_url = %config.redis_url,
        streams = ?streams,
        ttl_secs = config.ttl_secs,
        "ssmd-snap starting"
    );

    // Connect to NATS
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .expect("failed to connect to NATS");
    let js = async_nats::jetstream::new(nats_client);

    // Connect to Redis
    let redis_client =
        redis::Client::open(config.redis_url.as_str()).expect("invalid Redis URL");
    let redis_conn: redis::aio::MultiplexedConnection = redis_client
        .get_multiplexed_async_connection()
        .await
        .expect("failed to connect to Redis");

    // Test Redis connection
    let mut test_conn = redis_conn.clone();
    let _: String = redis::cmd("PING")
        .query_async(&mut test_conn)
        .await
        .expect("Redis PING failed");
    info!("connected to Redis");

    let metrics = Arc::new(Metrics::new());

    // Spawn a snap task per stream
    for stream_name in &streams {
        let stream_config = parse_stream(stream_name);
        info!(
            stream = %stream_config.stream_name,
            feed = %stream_config.feed,
            filter = %stream_config.filter_subject,
            "spawning snap consumer"
        );

        let js = js.clone();
        let redis_conn = redis_conn.clone();
        let ttl = config.ttl_secs;
        let m = metrics.clone();

        tokio::spawn(async move {
            snap::run_snap(js, redis_conn, stream_config, ttl, m).await;
        });
    }

    // Health + metrics HTTP server
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(prom_metrics))
        .with_state(metrics.clone());

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .expect("failed to bind metrics listener");
    info!(addr = %config.listen_addr, "health/metrics server listening");

    // Wait for shutdown signal
    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
        info!("received shutdown signal");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .expect("server error");

    info!("ssmd-snap stopped");
}

async fn healthz() -> &'static str {
    "ok"
}

async fn prom_metrics(State(metrics): State<Arc<Metrics>>) -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let metric_families = metrics.registry.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}
