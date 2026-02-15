//! ssmd-archiver binary entry point

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
use ssmd_archiver::manifest_io::update_manifest;
use ssmd_archiver::metrics::{ArchiverMetrics, StreamMetrics};
use ssmd_archiver::server::{run_server, ServerState};
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

    /// Address for health/metrics HTTP server
    #[arg(long, default_value = "0.0.0.0:8080")]
    health_addr: SocketAddr,
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
    let connected = Arc::new(AtomicBool::new(false));
    let last_message_epoch_secs = Arc::new(AtomicU64::new(0));

    // Create metrics
    let archiver_metrics = ArchiverMetrics::new(config.storage.feed.as_str());
    archiver_metrics.set_active_streams(config.nats.streams.len());

    // Spawn HTTP health/metrics server
    let server_state = ServerState::new(
        config.storage.feed.as_str(),
        connected.clone(),
        last_message_epoch_secs.clone(),
    );
    let health_addr = args.health_addr;
    tokio::spawn(async move {
        info!(%health_addr, "Starting health/metrics server");
        if let Err(e) = run_server(health_addr, server_state).await {
            error!(error = %e, "Health server failed");
        }
    });

    // Spawn a task per stream
    let mut tasks: JoinSet<Result<(), Box<dyn std::error::Error + Send + Sync>>> = JoinSet::new();

    for stream_config in config.nats.streams {
        let shutdown = shutdown.clone();
        let nats_url = Arc::clone(&nats_url);
        let base_path = Arc::clone(&base_path);
        let feed = Arc::clone(&feed);
        let rotation_interval = Arc::clone(&rotation_interval);
        let connected = connected.clone();
        let last_message_epoch_secs = last_message_epoch_secs.clone();
        let metrics = archiver_metrics.for_stream(&stream_config.name);

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
                metrics,
                connected,
                last_message_epoch_secs,
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
    metrics: StreamMetrics,
    connected: Arc<AtomicBool>,
    last_message_epoch_secs: Arc<AtomicU64>,
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
    connected.store(true, Ordering::SeqCst);

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
    let mut current_file_type_counts: HashMap<String, u64> = HashMap::new();

    // Sequence tracking (local — not worth Prometheus overhead)
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
                    for mut entry in writer.close()? {
                        entry.records_by_type = Some(std::mem::take(&mut current_file_type_counts));
                        completed_files.push(entry);
                    }
                    update_manifest(base_path, feed, &stream_name, &current_date, rotation_interval, &tickers, &message_types, &gaps, &completed_files)?;
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
                                metrics.inc_gap();
                            }

                            // Track stats via Prometheus metrics
                            metrics.inc_bytes(msg.payload().len() as u64);
                            if first_seq.is_none() {
                                first_seq = Some(msg.seq);
                            }
                            last_seq = msg.seq;
                            let epoch_secs = now.timestamp() as u64;
                            last_message_epoch_secs.store(epoch_secs, Ordering::Relaxed);
                            metrics.set_last_message_timestamp(epoch_secs as f64);

                            // Lightweight manifest field extraction (no full JSON tree)
                            let msg_type_for_count = match extract_manifest_fields(feed, msg.payload()) {
                                Some(fields) => {
                                    let mt = fields.msg_type;
                                    if let Some(ref t) = mt {
                                        message_types.insert(t.clone());
                                        metrics.inc_message(t);
                                    } else {
                                        metrics.inc_message("unknown");
                                    }
                                    if let Some(t) = fields.ticker {
                                        tickers.insert(t);
                                    }
                                    mt
                                }
                                None => {
                                    metrics.inc_parse_failure();
                                    metrics.inc_message("unknown");
                                    None
                                }
                            };

                            // Sampled validation: full parse 1-in-100 messages
                            #[allow(clippy::manual_is_multiple_of)]
                            if metrics.get_messages_total() % 100 == 0 {
                                if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(msg.payload()) {
                                    let vr = validator.validate(&parsed);
                                    if !vr.is_valid() {
                                        metrics.inc_validation_failure();
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
                                        for mut rotated_entry in rotated_entries {
                                            metrics.inc_files_rotated();
                                            rotated_entry.records_by_type = Some(std::mem::take(&mut current_file_type_counts));
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
                                    // Count current message type for the (possibly new) file
                                    if let Some(ref t) = msg_type_for_count {
                                        *current_file_type_counts.entry(t.clone()).or_insert(0) += 1;
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
                        // redelivery. DQ checks detect duplicates via _nats_seq.
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

                // Periodic stats log (read from Prometheus counters)
                if last_stats_time.elapsed() >= stats_interval {
                    info!(
                        stream_name = %stream_name,
                        feed = %feed,
                        messages = metrics.get_messages_total(),
                        bytes = metrics.get_bytes_total(),
                        validation_failures = metrics.get_validation_failures(),
                        parse_failures = metrics.get_parse_failures(),
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
    for mut entry in writer.close()? {
        entry.records_by_type = Some(std::mem::take(&mut current_file_type_counts));
        completed_files.push(entry);
    }
    update_manifest(
        base_path,
        feed,
        &stream_name,
        &current_date,
        rotation_interval,
        &tickers,
        &message_types,
        &gaps,
        &completed_files,
    )?;

    info!(stream_name = %stream_name, "Archive task stopped");
    Ok(())
}
