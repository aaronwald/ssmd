mod agg;
mod config;
mod consumer;
mod metrics;
mod store;

use std::sync::Arc;

use axum::{extract::State, routing::get, Router};
use tokio::sync::Mutex;
use tracing::info;

use agg::{parse_binance_trade, parse_kraken_trade, MinuteAggregator};
use config::Config;
use consumer::{run_subscription, Subscription};
use metrics::Metrics;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ssmd_bar_cache=info".into()),
        )
        .json()
        .init();

    // Fail-loud: from_env() panics with a clear message if NATS_URL/REDIS_URL
    // are missing, so the pod crashes immediately rather than limping.
    let config = Config::from_env();

    info!(
        nats_url = %config.nats_url,
        redis_url = %config.redis_url,
        kraken_subject = %config.kraken_subject,
        kraken_stream = %config.kraken_stream,
        binance_subject = %config.binance_subject,
        binance_stream = %config.binance_stream,
        ring = config.ring,
        ttl_secs = config.ttl_secs,
        listen_addr = %config.listen_addr,
        "ssmd-bar-cache starting"
    );

    // Connect to NATS (fail-loud: cannot do our job without it).
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .expect("failed to connect to NATS");
    let js = async_nats::jetstream::new(nats_client);

    // Connect to Redis (multiplexed) and verify with a PING before proceeding.
    let redis_client = redis::Client::open(config.redis_url.as_str()).expect("invalid Redis URL");
    let redis_conn: redis::aio::MultiplexedConnection = redis_client
        .get_multiplexed_async_connection()
        .await
        .expect("failed to connect to Redis");

    let mut ping_conn = redis_conn.clone();
    let pong: String = redis::cmd("PING")
        .query_async(&mut ping_conn)
        .await
        .expect("Redis PING failed");
    if pong != "PONG" {
        panic!("unexpected Redis PING response: {pong:?}");
    }
    info!("connected to Redis");

    // Crash on sustained Redis loss (every 30s watchdog) per arch rules.
    ssmd_middleware::redis_health::spawn_redis_health_check(redis_conn.clone());

    let metrics = Arc::new(Metrics::new());

    // One aggregator shared by all feeds; per-symbol state is single-writer
    // behind the Mutex (see consumer.rs).
    let aggregator = Arc::new(Mutex::new(MinuteAggregator::new()));

    // The subscriptions differ only by stream/subject, parser, and feed label —
    // this is the entirety of the multi-feed routing.
    let subscriptions = vec![
        Subscription {
            stream_name: config.kraken_stream.clone(),
            filter_subject: config.kraken_subject.clone(),
            feed: "kraken-spot".to_string(),
            parse: parse_kraken_trade,
        },
        Subscription {
            stream_name: config.binance_stream.clone(),
            filter_subject: config.binance_subject.clone(),
            feed: "binance".to_string(),
            parse: parse_binance_trade,
        },
    ];

    for sub in subscriptions {
        let js = js.clone();
        let redis_conn = redis_conn.clone();
        let agg = aggregator.clone();
        let metrics = metrics.clone();
        let ring = config.ring;
        let ttl = config.ttl_secs;

        tokio::spawn(async move {
            run_subscription(js, redis_conn, agg, sub, ring, ttl, metrics).await;
        });
    }

    // Health + metrics HTTP server.
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(prom_metrics))
        .with_state(metrics.clone());

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .expect("failed to bind metrics listener");
    info!(addr = %config.listen_addr, "health/metrics server listening");

    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
        info!("received shutdown signal");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .expect("server error");

    info!("ssmd-bar-cache stopped");
}

async fn healthz() -> &'static str {
    "ok"
}

async fn prom_metrics(State(metrics): State<Arc<Metrics>>) -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let metric_families = metrics.registry.gather();
    let mut buffer = Vec::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .expect("encode metrics");
    String::from_utf8(buffer).expect("metrics are valid UTF-8")
}
