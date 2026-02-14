//! ssmd-archiver binary entry point

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use clap::Parser;
use tokio::signal::unix::{signal, SignalKind};
use tokio::task::JoinSet;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_archiver::config::StreamConfig;
use ssmd_archiver::manifest::{FileEntry, Gap};
use ssmd_archiver::manifest_io::{update_manifest, write_manifest};
use ssmd_archiver::subscriber::Subscriber;
use ssmd_archiver::validation::{extract_manifest_fields, MessageValidator};
use ssmd_archiver::writer::{ArchiveOutput, ArchiveWriter};
use ssmd_archiver::Config;

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
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    // Load configuration
    let config = Config::load(&args.config).map_err(|e| {
        error!(error = %e, "Failed to load config");
        e
    })?;

    let rotation_duration = config.rotation.parse_interval()?;

    info!(
        nats_url = %config.nats.url,
        streams = config.nats.streams.len(),
        rotation = %config.rotation.interval,
        storage = ?config.storage.path,
        "Starting archiver"
    );

    // Create shared state
    let shutdown = CancellationToken::new();
    let nats_url = Arc::new(config.nats.url.clone());
    let base_path = Arc::new(config.storage.path.clone());
    let feed = Arc::new(config.storage.feed.clone());
    let rotation_interval = Arc::new(config.rotation.interval.clone());

    // Spawn a task per stream
    let mut tasks: JoinSet<Result<(), Box<dyn std::error::Error + Send + Sync>>> = JoinSet::new();

    for stream_config in config.nats.streams {
        let shutdown = shutdown.clone();
        let nats_url = Arc::clone(&nats_url);
        let base_path = Arc::clone(&base_path);
        let feed = Arc::clone(&feed);
        let rotation_interval = Arc::clone(&rotation_interval);

        info!(
            stream = %stream_config.stream,
            name = %stream_config.name,
            filter = %stream_config.filter,
            "Spawning archive task"
        );

        tasks.spawn(async move {
            archive_stream(
                &nats_url,
                stream_config,
                &base_path,
                &feed,
                &rotation_interval,
                rotation_duration,
                shutdown,
            )
            .await
        });
    }

    // Set up signal handlers for graceful shutdown
    let mut sigterm =
        signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");
    let mut sigint =
        signal(SignalKind::interrupt()).expect("Failed to create SIGINT handler");

    info!("Archiver running, waiting for SIGTERM/SIGINT to stop");

    // Wait for signal or task failure
    tokio::select! {
        _ = sigterm.recv() => {
            info!("SIGTERM received, shutting down gracefully");
            shutdown.cancel();
        }
        _ = sigint.recv() => {
            info!("SIGINT received, shutting down gracefully");
            shutdown.cancel();
        }
        result = tasks.join_next() => {
            match result {
                Some(Ok(Ok(()))) => {
                    info!("Task completed successfully, shutting down all tasks");
                }
                Some(Ok(Err(e))) => {
                    error!(error = %e, "Task failed, shutting down all tasks");
                }
                Some(Err(e)) => {
                    error!(error = %e, "Task panicked, shutting down all tasks");
                }
                None => {
                    info!("All tasks completed");
                }
            }
            shutdown.cancel();
        }
    }

    // Wait for all remaining tasks to finish
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => info!("Task shutdown complete"),
            Ok(Err(e)) => error!(error = %e, "Task failed during shutdown"),
            Err(e) => error!(error = %e, "Task panicked during shutdown"),
        }
    }

    // Write sync-ready marker file to signal that all data has been flushed
    let marker_path = base_path.join(".sync-ready");
    match std::fs::write(&marker_path, Utc::now().to_rfc3339()) {
        Ok(()) => info!(path = ?marker_path, "Wrote sync-ready marker"),
        Err(e) => error!(error = %e, path = ?marker_path, "Failed to write sync-ready marker"),
    }

    info!("Archiver stopped");
    Ok(())
}

/// Archive messages from a single stream
#[allow(clippy::too_many_arguments)]
async fn archive_stream(
    nats_url: &str,
    stream_config: StreamConfig,
    base_path: &Path,
    feed: &str,
    rotation_interval: &str,
    rotation_duration: Duration,
    shutdown: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stream_name = stream_config.name.clone();
    let rotation_minutes = (rotation_duration.as_secs() / 60) as u32;

    info!(
        stream = %stream_config.stream,
        stream_name = %stream_name,
        "Connecting to NATS"
    );

    // Connect to NATS
    let mut subscriber = Subscriber::connect(nats_url, &stream_config).await?;

    // Create JSONL.gz writer
    let mut writer = ArchiveWriter::new(
        base_path.to_path_buf(),
        feed.to_string(),
        stream_name.clone(),
        rotation_minutes,
    );

    // Create message validator for field-presence checks
    let validator = MessageValidator::new(feed);

    // Track manifest data
    let mut tickers: HashSet<String> = HashSet::new();
    let mut message_types: HashSet<String> = HashSet::new();
    let mut gaps: Vec<Gap> = Vec::new();
    let mut completed_files: Vec<FileEntry> = Vec::new();
    let mut current_date = Utc::now().format("%Y-%m-%d").to_string();

    // Stats tracking
    let mut total_messages: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut validation_failures: u64 = 0;
    let mut parse_failures: u64 = 0;
    let mut first_seq: Option<u64> = None;
    let mut last_seq: u64 = 0;
    let mut last_stats_time = std::time::Instant::now();
    let stats_interval = Duration::from_secs(30);

    // Fetch interval
    let mut fetch_interval = interval(Duration::from_millis(100));

    info!(stream_name = %stream_name, "Archive task running");

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                info!(stream_name = %stream_name, "Shutdown signal received");
                break;
            }
            _ = fetch_interval.tick() => {
                let now = Utc::now();
                let date = now.format("%Y-%m-%d").to_string();

                // Check for day rollover
                if date != current_date {
                    info!(stream_name = %stream_name, old = %current_date, new = %date, "Day rollover, writing final manifest");
                    write_manifest(base_path, feed, &stream_name, &current_date, rotation_interval, &mut writer, &tickers, &message_types, &gaps, &mut completed_files)?;
                    tickers.clear();
                    message_types.clear();
                    gaps.clear();
                    completed_files.clear();
                    current_date = date;
                }

                // Fetch messages
                match subscriber.fetch(100).await {
                    Ok(messages) => {
                        let mut pending_acks = Vec::with_capacity(messages.len());

                        for msg in messages {
                            // Check for gap
                            if let Some((after_seq, missing)) = msg.gap {
                                warn!(stream_name = %stream_name, after_seq = after_seq, missing = missing, "Recording gap");
                                gaps.push(Gap {
                                    after_seq,
                                    missing_count: missing,
                                    detected_at: now,
                                });
                            }

                            // Track stats
                            total_messages += 1;
                            total_bytes += msg.payload().len() as u64;
                            if first_seq.is_none() {
                                first_seq = Some(msg.seq);
                            }
                            last_seq = msg.seq;

                            // Lightweight manifest field extraction (no full JSON tree)
                            match extract_manifest_fields(feed, msg.payload()) {
                                Some(fields) => {
                                    if let Some(t) = fields.msg_type {
                                        message_types.insert(t);
                                    }
                                    if let Some(t) = fields.ticker {
                                        tickers.insert(t);
                                    }
                                }
                                None => {
                                    parse_failures += 1;
                                }
                            }

                            // Sampled validation: full parse 1-in-100 messages
                            if total_messages.is_multiple_of(100) {
                                if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(msg.payload()) {
                                    let vr = validator.validate(&parsed);
                                    if !vr.is_valid() {
                                        validation_failures += 1;
                                        warn!(
                                            stream_name = %stream_name,
                                            msg_type = ?vr.message_type,
                                            missing = ?vr.missing_fields,
                                            seq = msg.seq,
                                            "Validation failure: missing required fields"
                                        );
                                    }
                                }
                            }

                            // Write to archive (returns rotated FileEntries on rotation)
                            let seq = msg.seq;
                            match writer.write(msg.payload(), seq, now) {
                                Ok(rotated_entries) => {
                                    pending_acks.push(msg);
                                    if !rotated_entries.is_empty() {
                                        for rotated_entry in rotated_entries {
                                            info!(
                                                stream_name = %stream_name,
                                                file = %rotated_entry.name,
                                                records = rotated_entry.records,
                                                nats_start_seq = rotated_entry.nats_start_seq,
                                                nats_end_seq = rotated_entry.nats_end_seq,
                                                "File rotated"
                                            );
                                            completed_files.push(rotated_entry);
                                        }
                                        if let Err(e) = update_manifest(base_path, feed, &stream_name, &current_date, rotation_interval, &tickers, &message_types, &gaps, &completed_files) {
                                            error!(stream_name = %stream_name, error = %e, "Failed to update manifest after rotation");
                                        }
                                    }
                                }
                                Err(e) => {
                                    // Don't ack - message will be redelivered by NATS
                                    warn!(stream_name = %stream_name, error = %e, seq = seq, "Failed to write message, will be redelivered");
                                }
                            }
                        }

                        // Batch ack: flush to OS page cache first, then ack.
                        // At-least-once: crash between flush and ack causes
                        // redelivery. Downstream parquet-gen dedup handles this.
                        if !pending_acks.is_empty() {
                            match writer.flush() {
                                Ok(()) => {
                                    for msg in pending_acks {
                                        let seq = msg.seq;
                                        if let Err(e) = msg.ack().await {
                                            error!(stream_name = %stream_name, error = %e, seq = seq, "Failed to ack message");
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(
                                        stream_name = %stream_name,
                                        error = %e,
                                        pending = pending_acks.len(),
                                        "Failed to flush archive data, skipping acks so messages are redelivered"
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!(stream_name = %stream_name, error = %e, "Failed to fetch messages");
                    }
                }

                // Periodic stats log
                if last_stats_time.elapsed() >= stats_interval {
                    info!(
                        stream_name = %stream_name,
                        feed = %feed,
                        messages = total_messages,
                        bytes = total_bytes,
                        validation_failures = validation_failures,
                        parse_failures = parse_failures,
                        tickers = tickers.len(),
                        nats_start_seq = first_seq.unwrap_or(0),
                        nats_end_seq = last_seq,
                        gaps = gaps.len(),
                        "Archiver stats"
                    );
                    last_stats_time = std::time::Instant::now();
                }
            }
        }
    }

    // Final cleanup
    info!(stream_name = %stream_name, "Writing final manifest");
    write_manifest(
        base_path,
        feed,
        &stream_name,
        &current_date,
        rotation_interval,
        &mut writer,
        &tickers,
        &message_types,
        &gaps,
        &mut completed_files,
    )?;

    info!(stream_name = %stream_name, "Archive task stopped");
    Ok(())
}
