mod agg;
mod config;

use config::Config;
use tracing::info;

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
        massive_subject = %config.massive_subject,
        massive_stream = %config.massive_stream,
        kraken_subject = %config.kraken_subject,
        kraken_stream = %config.kraken_stream,
        ring = config.ring,
        ttl_secs = config.ttl_secs,
        listen_addr = %config.listen_addr,
        "ssmd-bar-cache starting"
    );

    // TODO Tasks 3-4: NATS JetStream consumers (massive + kraken) feeding the
    // MinuteAggregator, and a Redis writer for the rolling 1-hour ring, plus the
    // health/metrics HTTP server. Connection verification (NATS reachable,
    // Redis PING, redis_health watchdog) is wired here in Task 3 — NOT in this
    // skeleton. No NATS/Redis I/O yet.
}
