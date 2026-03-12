use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};
use clap::Parser;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
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
        &config.publication_name,
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

        match replication.poll_changes().await {
            Ok(events) => {
                consecutive_failures = 0; // Reset on success

                for event in events {
                    // Skip tables we don't need CDC for
                    if !publish_tables.contains(event.table.as_str()) {
                        events_skipped += 1;
                        metrics::CDC_EVENTS_SKIPPED.inc();
                        continue;
                    }

                    if let Err(e) = publisher.publish(&event).await {
                        tracing::error!(error = ?e, table = %event.table, "Failed to publish event");
                        metrics::CDC_PUBLISH_ERRORS.with_label_values(&[&event.table]).inc();
                    } else {
                        events_published += 1;
                        metrics::CDC_EVENTS_PUBLISHED.with_label_values(&[&event.table]).inc();
                        metrics::CDC_LAST_PUBLISH_TIMESTAMP.set(chrono::Utc::now().timestamp() as f64);
                        if events_published % 100 == 0 {
                            tracing::info!(total = events_published, skipped = events_skipped, "Events published");
                        }
                    }
                }
            }
            Err(e) => {
                consecutive_failures += 1;
                metrics::CDC_POLL_ERRORS.inc();
                tracing::error!(
                    error = ?e,
                    consecutive_failures = consecutive_failures,
                    "Failed to poll changes"
                );

                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    tracing::error!("Max consecutive failures reached, exiting for restart");
                    return Err(e.into());
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}
