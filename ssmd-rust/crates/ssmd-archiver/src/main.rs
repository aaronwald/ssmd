//! ssmd-archiver binary entry point

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::Utc;
use clap::Parser;
use tokio::signal::unix::{signal, SignalKind};
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
    let mut completed_files: Vec<ssmd_archiver::manifest::FileEntry> = Vec::new();
    let mut current_date = Utc::now().format("%Y-%m-%d").to_string();

    // Stats tracking
    let mut total_messages: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut first_seq: Option<u64> = None;
    let mut last_seq: u64 = 0;
    let mut last_stats_time = std::time::Instant::now();
    let stats_interval = Duration::from_secs(30);

    // Fetch interval
    let mut fetch_interval = interval(Duration::from_millis(100));

    // Set up signal handlers for graceful shutdown
    let mut sigterm = signal(SignalKind::terminate())
        .expect("Failed to create SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt())
        .expect("Failed to create SIGINT handler");

    info!("Archiver running, waiting for SIGTERM/SIGINT to stop");

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("SIGTERM received, shutting down gracefully");
                break;
            }
            _ = sigint.recv() => {
                info!("SIGINT received, shutting down gracefully");
                break;
            }
            _ = fetch_interval.tick() => {
                let now = Utc::now();
                let date = now.format("%Y-%m-%d").to_string();

                // Check for day rollover
                if date != current_date {
                    info!(old = %current_date, new = %date, "Day rollover, writing final manifest");
                    write_manifest(&config.storage.path, &feed, &current_date, &config.rotation.interval, &mut writer, &tickers, &message_types, &gaps, &mut completed_files)?;
                    tickers.clear();
                    message_types.clear();
                    gaps.clear();
                    completed_files.clear();
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

                            // Track stats
                            total_messages += 1;
                            total_bytes += msg.data.len() as u64;
                            if first_seq.is_none() {
                                first_seq = Some(msg.seq);
                            }
                            last_seq = msg.seq;

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

                            // Write to file (returns Some(FileEntry) on rotation)
                            let seq = msg.seq;
                            match writer.write(&msg.data, seq, now) {
                                Ok(Some(rotated_entry)) => {
                                    // Ack after successful write
                                    if let Err(e) = msg.ack().await {
                                        error!(error = %e, seq = seq, "Failed to ack message");
                                    }
                                    // File was rotated - update manifest immediately
                                    info!(
                                        file = %rotated_entry.name,
                                        records = rotated_entry.records,
                                        seq_range = %format!("{}-{}", rotated_entry.nats_start_seq, rotated_entry.nats_end_seq),
                                        "File rotated"
                                    );
                                    completed_files.push(rotated_entry);
                                    if let Err(e) = update_manifest(&config.storage.path, &feed, &current_date, &config.rotation.interval, &tickers, &message_types, &gaps, &completed_files) {
                                        error!(error = %e, "Failed to update manifest after rotation");
                                    }
                                }
                                Ok(None) => {
                                    // Ack after successful write
                                    if let Err(e) = msg.ack().await {
                                        error!(error = %e, seq = seq, "Failed to ack message");
                                    }
                                }
                                Err(e) => {
                                    // Don't ack - message will be redelivered by NATS
                                    warn!(error = %e, seq = seq, "Failed to write message, will be redelivered");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to fetch messages");
                    }
                }

                // Periodic stats log
                if last_stats_time.elapsed() >= stats_interval {
                    info!(
                        feed = %feed,
                        messages = total_messages,
                        bytes = total_bytes,
                        tickers = tickers.len(),
                        seq_range = %format!("{}-{}", first_seq.unwrap_or(0), last_seq),
                        gaps = gaps.len(),
                        "Archiver stats"
                    );
                    last_stats_time = std::time::Instant::now();
                }
            }
        }
    }

    // Final cleanup
    info!("Writing final manifest");
    write_manifest(&config.storage.path, &feed, &current_date, &config.rotation.interval, &mut writer, &tickers, &message_types, &gaps, &mut completed_files)?;

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

/// Update manifest with completed files (called on every rotation)
#[allow(clippy::too_many_arguments)]
fn update_manifest(
    base_path: &Path,
    feed: &str,
    date: &str,
    rotation_interval: &str,
    tickers: &HashSet<String>,
    message_types: &HashSet<String>,
    gaps: &[Gap],
    completed_files: &[ssmd_archiver::manifest::FileEntry],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut manifest = Manifest::new(feed, date, rotation_interval);
    manifest.files = completed_files.to_vec();
    manifest.tickers = tickers.iter().cloned().collect();
    manifest.message_types = message_types.iter().cloned().collect();
    manifest.gaps = gaps.to_vec();
    manifest.has_gaps = !gaps.is_empty();

    let manifest_path = base_path.join(feed).join(date).join("manifest.json");
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, manifest_json)?;

    info!(path = ?manifest_path, files = completed_files.len(), "Updated manifest");
    Ok(())
}

/// Write final manifest (called on shutdown/day rollover, closes current file)
#[allow(clippy::too_many_arguments)]
fn write_manifest(
    base_path: &Path,
    feed: &str,
    date: &str,
    rotation_interval: &str,
    writer: &mut ArchiveWriter,
    tickers: &HashSet<String>,
    message_types: &HashSet<String>,
    gaps: &[Gap],
    completed_files: &mut Vec<ssmd_archiver::manifest::FileEntry>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Close current file and add to completed files
    if let Some(entry) = writer.close()? {
        completed_files.push(entry);
    }

    // Write manifest with all completed files
    update_manifest(base_path, feed, date, rotation_interval, tickers, message_types, gaps, completed_files)
}
