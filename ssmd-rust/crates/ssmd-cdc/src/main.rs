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

        match replication.peek_changes(args.peek_batch_limit).await {
            Ok(events) => {
                consecutive_failures = 0; // Reset on success
                let batch_len = events.len();

                if !events.is_empty() {
                    // Track the last LSN we successfully publish — we advance to this.
                    let mut last_published_lsn: Option<String> = None;
                    let mut batch_failed = false;

                    for event in &events {
                        // Skip tables we don't need CDC for — but still track LSN
                        if !publish_tables.contains(event.table.as_str()) {
                            events_skipped += 1;
                            metrics::CDC_EVENTS_SKIPPED.inc();
                            last_published_lsn = Some(event.lsn.clone());
                            continue;
                        }

                        if let Err(e) = publisher.publish(event).await {
                            tracing::error!(error = ?e, table = %event.table, lsn = %event.lsn, "Failed to publish event — aborting batch");
                            metrics::CDC_PUBLISH_ERRORS.with_label_values(&[&event.table]).inc();
                            batch_failed = true;
                            break;
                        } else {
                            events_published += 1;
                            metrics::CDC_EVENTS_PUBLISHED.with_label_values(&[&event.table]).inc();
                            metrics::CDC_LAST_PUBLISH_TIMESTAMP.set(chrono::Utc::now().timestamp() as f64);
                            last_published_lsn = Some(event.lsn.clone());
                            if events_published % 100 == 0 {
                                tracing::info!(total = events_published, skipped = events_skipped, "Events published");
                            }
                        }
                    }

                    // Crash on advance failure — a CDC that peeks without
                    // advancing will accumulate unbounded WAL and never recover.
                    if let Some(ref lsn) = last_published_lsn {
                        replication.advance_slot(lsn).await?;
                    }

                    if batch_failed {
                        // Back off before retrying — NATS dedup prevents duplicates on re-peek
                        next_sleep = Duration::from_secs(2);
                    } else if batch_len as i64 >= args.peek_batch_limit {
                        // Full batch — more events waiting, poll again immediately
                        next_sleep = Duration::ZERO;
                    }
                }
            }
            Err(e) => {
                consecutive_failures += 1;
                metrics::CDC_POLL_ERRORS.inc();
                tracing::error!(
                    error = ?e,
                    consecutive_failures = consecutive_failures,
                    "Failed to peek changes"
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
