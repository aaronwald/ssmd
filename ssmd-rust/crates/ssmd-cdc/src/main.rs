use clap::Parser;
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_cdc::{config::Config, publisher::Publisher, replication::ReplicationSlot};

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
        "Starting ssmd-cdc"
    );

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

    // Main polling loop
    let poll_interval = Duration::from_millis(args.poll_interval_ms);
    let mut events_published: u64 = 0;

    loop {
        match replication.poll_changes().await {
            Ok(events) => {
                for event in events {
                    if let Err(e) = publisher.publish(&event).await {
                        tracing::error!(error = %e, table = %event.table, "Failed to publish event");
                    } else {
                        events_published += 1;
                        if events_published.is_multiple_of(100) {
                            tracing::info!(total = events_published, "Events published");
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to poll changes");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}
