use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};
use clap::Parser;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_cdc::{config::Config, metrics, publisher::Publisher, replication::ReplicationSlot};

#[derive(Parser)]
#[command(name = "ssmd-cdc")]
#[command(about = "PostgreSQL CDC to NATS publisher")]
struct Args {
    /// Poll interval in milliseconds
    #[arg(long, env = "POLL_INTERVAL_MS", default_value = "100")]
    poll_interval_ms: u64,

    /// NATS stream name
    #[arg(long, env = "NATS_STREAM", default_value = "SECMASTER_CDC")]
    stream_name: String,

    /// Maximum events to peek per poll cycle (bounds memory usage)
    #[arg(long, env = "PEEK_BATCH_LIMIT", default_value = "1000")]
    peek_batch_limit: i64,

    /// Health/metrics server address
    #[arg(long, env = "HEALTH_ADDR", default_value = "0.0.0.0:8080")]
    health_addr: SocketAddr,
}

async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn metrics_handler() -> impl IntoResponse {
    match metrics::encode_metrics() {
        Ok(body) => (
            StatusCode::OK,
            [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
            body,
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "text/plain; charset=utf-8")],
            format!("Failed to encode metrics: {}", e),
        ),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let config = Config::from_env()?;

    tracing::info!(
        database_url = %config.database_url.split('@').next_back().unwrap_or("***"),
        nats_url = %config.nats_url,
        slot = %config.slot_name,
        health_addr = %args.health_addr,
        "Starting ssmd-cdc"
    );

    // Spawn health/metrics HTTP server
    let health_addr = args.health_addr;
    tokio::spawn(async move {
        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/metrics", get(metrics_handler));
        let listener = TcpListener::bind(health_addr).await.expect("Failed to bind health server");
        tracing::info!(%health_addr, "Health/metrics server started");
        axum::serve(listener, app).await.expect("Health server failed");
    });

    // Pre-initialize publish error metric so GMP discovers the metric name
    metrics::CDC_PUBLISH_ERRORS.with_label_values(&["_init"]);

    // Connect to NATS and ensure stream exists
    let publisher = Publisher::new(&config.nats_url, &args.stream_name).await?;
    publisher.ensure_stream().await?;

    // Connect to PostgreSQL and ensure replication slot exists
    let replication = ReplicationSlot::connect(
        &config.database_url,
        &config.slot_name,
    ).await?;
    replication.ensure_slot().await?;

    // Spawn Postgres health check (every 30s — crash if Postgres is unreachable)
    ssmd_middleware::postgres_health::spawn_postgres_health_check(replication.pool().clone());

    let lsn = replication.current_lsn().await?;
    tracing::info!(lsn = %lsn, "Starting from LSN");

    // Tables to publish CDC events for (others are ignored)
    let publish_tables: std::collections::HashSet<&str> = config.tables.iter().map(|s| s.as_str()).collect();

    // Main polling loop
    let poll_interval = Duration::from_millis(args.poll_interval_ms);
    let mut events_published: u64 = 0;
    let mut events_skipped: u64 = 0;
    let mut consecutive_failures: u32 = 0;
    let mut poll_count: u64 = 0;
    const MAX_CONSECUTIVE_FAILURES: u32 = 5;

    let mut shutdown_rx = signal::unix::signal(signal::unix::SignalKind::terminate())?;

    loop {
        poll_count += 1;
        metrics::CDC_POLLS_TOTAL.inc();

        // Log heartbeat every 10 minutes (6000 polls at 100ms interval)
        if poll_count % 6000 == 0 {
            tracing::info!(
                polls = poll_count,
                published = events_published,
                skipped = events_skipped,
                "CDC heartbeat"
            );
        }

        // Default sleep duration — overridden by error/backoff paths below
        let mut next_sleep = poll_interval;

        // Use get_changes (not peek+advance) — atomically consumes changes.
        // If we crash after get but before NATS publish, those changes are lost.
        // This is acceptable: NATS dedup handles re-delivery, and cache warmer
        // does a full DB refresh on restart.
        match replication.get_changes(args.peek_batch_limit).await {
            Ok(events) => {
                consecutive_failures = 0;
                let batch_len = events.len();

                if !events.is_empty() {
                    for event in &events {
                        if !publish_tables.contains(event.table.as_str()) {
                            events_skipped += 1;
                            metrics::CDC_EVENTS_SKIPPED.inc();
                            continue;
                        }

                        if let Err(e) = publisher.publish(event).await {
                            tracing::error!(error = ?e, table = %event.table, lsn = %event.lsn, "Failed to publish — crashing for restart");
                            metrics::CDC_PUBLISH_ERRORS.with_label_values(&[&event.table]).inc();
                            replication.close();
                            return Err(e.into());
                        }

                        events_published += 1;
                        metrics::CDC_EVENTS_PUBLISHED.with_label_values(&[&event.table]).inc();
                        metrics::CDC_LAST_PUBLISH_TIMESTAMP.set(chrono::Utc::now().timestamp() as f64);
                        if events_published % 100 == 0 {
                            tracing::info!(total = events_published, skipped = events_skipped, "Events published");
                        }
                    }

                    if batch_len as i64 >= args.peek_batch_limit {
                        next_sleep = Duration::ZERO;
                    }
                }
            }
            Err(e) => {
                consecutive_failures += 1;
                metrics::CDC_POLL_ERRORS.inc();

                // Detect statement timeout (SqlState E57014) — WAL backlog too large to decode.
                // Recovery: advance the slot to current LSN, discarding the backlog.
                // This is safe because connector reloads all markets from DB on restart,
                // and cache warmer does a full Redis refresh.
                let is_statement_timeout = matches!(&e, ssmd_cdc::Error::Postgres(pg_err)
                    if pg_err.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED));

                if is_statement_timeout {
                    tracing::warn!(
                        consecutive_failures = consecutive_failures,
                        "Statement timeout — WAL backlog too large to decode, advancing slot to current LSN"
                    );
                    match replication.current_lsn().await {
                        Ok(current_lsn) => {
                            if let Err(adv_err) = replication.advance_slot(&current_lsn).await {
                                tracing::error!(error = ?adv_err, "Failed to advance slot — crashing for restart");
                                replication.close();
                                return Err(adv_err.into());
                            }
                            tracing::warn!(lsn = %current_lsn, "Slot advanced past backlog — resuming normal operation");
                            consecutive_failures = 0;
                            continue;
                        }
                        Err(lsn_err) => {
                            tracing::error!(error = ?lsn_err, "Failed to get current LSN for slot advance");
                        }
                    }
                }

                tracing::error!(
                    error = ?e,
                    consecutive_failures = consecutive_failures,
                    "Failed to get changes"
                );

                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    tracing::error!("Max consecutive failures reached, exiting for restart");
                    replication.close();
                    return Err(e.into());
                }

                next_sleep = Duration::from_secs(5);
            }
        }

        // Single sleep-or-shutdown select at the bottom of every iteration
        tokio::select! {
            biased;
            _ = shutdown_rx.recv() => {
                tracing::info!("Received SIGTERM — releasing replication slot");
                replication.close();
                return Ok(());
            }
            _ = tokio::time::sleep(next_sleep) => {}
        }
    }
}
