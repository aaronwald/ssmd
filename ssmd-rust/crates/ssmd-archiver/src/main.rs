//! ssmd-archiver binary entry point

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;
use tokio::signal;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_archiver::{Config, manifest::{Gap, Manifest}, subscriber::Subscriber, writer::ArchiveWriter};

#[derive(Parser, Debug)]
#[command(name = "ssmd-archiver")]
#[command(about = "NATS to file archiver for SSMD market data")]
struct Args {
    /// Path to archiver configuration file
    #[arg(short, long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Load configuration
    let config = Config::load(&args.config).map_err(|e| {
        error!(error = %e, "Failed to load config");
        e
    })?;

    info!(
        nats_url = %config.nats.url,
        stream = %config.nats.stream,
        filter = %config.nats.filter,
        rotation = %config.rotation.interval,
        storage = ?config.storage.path,
        "Starting archiver"
    );

    let rotation_duration = config.rotation.parse_interval()?;
    let rotation_minutes = (rotation_duration.as_secs() / 60) as u32;

    // Extract feed name from filter (e.g., "prod.kalshi.json.>" -> "kalshi")
    let feed = extract_feed_from_filter(&config.nats.filter)
        .ok_or("Could not extract feed from filter")?;

    // Connect to NATS
    let mut subscriber = Subscriber::connect(&config.nats).await?;

    // Create writer
    let mut writer = ArchiveWriter::new(
        config.storage.path.clone(),
        feed.clone(),
        rotation_minutes,
    );

    // Track manifest data
    let mut tickers: HashSet<String> = HashSet::new();
    let mut message_types: HashSet<String> = HashSet::new();
    let mut gaps: Vec<Gap> = Vec::new();
    let mut current_date = Utc::now().format("%Y-%m-%d").to_string();

    // Fetch interval
    let mut fetch_interval = interval(Duration::from_millis(100));

    info!("Archiver running, press Ctrl+C to stop");

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Shutdown signal received");
                break;
            }
            _ = fetch_interval.tick() => {
                let now = Utc::now();
                let date = now.format("%Y-%m-%d").to_string();

                // Check for day rollover
                if date != current_date {
                    info!(old = %current_date, new = %date, "Day rollover, writing manifest");
                    write_manifest(&config.storage.path, &feed, &current_date, &config.rotation.interval, &mut writer, &tickers, &message_types, &gaps)?;
                    tickers.clear();
                    message_types.clear();
                    gaps.clear();
                    current_date = date;
                }

                // Fetch messages
                match subscriber.fetch(100).await {
                    Ok(messages) => {
                        for msg in messages {
                            // Check for gap
                            if let Some((after_seq, missing)) = subscriber.check_gap(msg.seq) {
                                warn!(after_seq = after_seq, missing = missing, "Recording gap");
                                gaps.push(Gap {
                                    after_seq,
                                    missing_count: missing,
                                    detected_at: now,
                                });
                            }

                            // Extract ticker and type for manifest
                            if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&msg.data) {
                                if let Some(msg_type) = parsed.get("type").and_then(|v| v.as_str()) {
                                    message_types.insert(msg_type.to_string());
                                }
                                if let Some(inner) = parsed.get("msg") {
                                    if let Some(ticker) = inner.get("market_ticker").and_then(|v| v.as_str()) {
                                        tickers.insert(ticker.to_string());
                                    }
                                }
                            }

                            // Write to file
                            if let Err(e) = writer.write(&msg.data, msg.seq, now) {
                                error!(error = %e, "Failed to write message");
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to fetch messages");
                    }
                }
            }
        }
    }

    // Final cleanup
    info!("Writing final manifest");
    write_manifest(&config.storage.path, &feed, &current_date, &config.rotation.interval, &mut writer, &tickers, &message_types, &gaps)?;

    info!("Archiver stopped");
    Ok(())
}

fn extract_feed_from_filter(filter: &str) -> Option<String> {
    // Filter format: "{env}.{feed}.json.>" or "{env}.{feed}.json.{type}.>"
    let parts: Vec<&str> = filter.split('.').collect();
    if parts.len() >= 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

fn write_manifest(
    base_path: &PathBuf,
    feed: &str,
    date: &str,
    rotation_interval: &str,
    writer: &mut ArchiveWriter,
    tickers: &HashSet<String>,
    message_types: &HashSet<String>,
    gaps: &[Gap],
) -> Result<(), Box<dyn std::error::Error>> {
    // Close current file and get entry
    let file_entry = writer.close()?;

    let mut manifest = Manifest::new(feed, date, rotation_interval);
    if let Some(entry) = file_entry {
        manifest.files.push(entry);
    }
    manifest.tickers = tickers.iter().cloned().collect();
    manifest.message_types = message_types.iter().cloned().collect();
    manifest.gaps = gaps.to_vec();
    manifest.has_gaps = !gaps.is_empty();

    // Write manifest
    let manifest_path = base_path.join(feed).join(date).join("manifest.json");
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, manifest_json)?;

    info!(path = ?manifest_path, "Wrote manifest");
    Ok(())
}
