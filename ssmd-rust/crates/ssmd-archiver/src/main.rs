//! ssmd-archiver binary entry point

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use clap::Parser;
use tokio::signal::unix::{signal, SignalKind};
use tokio::task::JoinSet;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_archiver::config::{OutputFormat, StreamConfig};
use ssmd_archiver::manifest::{FileEntry, Gap, Manifest};
use ssmd_archiver::parquet_writer::ParquetWriter;
use ssmd_archiver::subscriber::Subscriber;
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

/// Writes to both JSONL.gz and Parquet simultaneously.
struct MultiWriter {
    jsonl: ArchiveWriter,
    parquet: ParquetWriter,
}

impl MultiWriter {
    fn new(
        base_path: PathBuf,
        feed: String,
        stream_name: String,
        rotation_minutes: u32,
    ) -> Self {
        Self {
            jsonl: ArchiveWriter::new(
                base_path.clone(),
                feed.clone(),
                stream_name.clone(),
                rotation_minutes,
            ),
            parquet: ParquetWriter::new(base_path, feed, stream_name),
        }
    }
}

impl ArchiveOutput for MultiWriter {
    fn write(
        &mut self,
        data: &[u8],
        seq: u64,
        now: DateTime<Utc>,
    ) -> Result<Vec<FileEntry>, ssmd_archiver::ArchiverError> {
        // Write to JSONL but discard its entries — parquet has richer metadata
        self.jsonl.write(data, seq, now)?;
        self.parquet.write(data, seq, now)
    }

    fn close(&mut self) -> Result<Vec<FileEntry>, ssmd_archiver::ArchiverError> {
        self.jsonl.close()?;
        self.parquet.close()
    }
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

    let rotation_duration = config.rotation.parse_interval()?;
    let output_format = config.storage.format;

    info!(
        nats_url = %config.nats.url,
        streams = config.nats.streams.len(),
        rotation = %config.rotation.interval,
        storage = ?config.storage.path,
        format = %output_format,
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
        let format = output_format;

        info!(
            stream = %stream_config.stream,
            name = %stream_config.name,
            filter = %stream_config.filter,
            format = %format,
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
                format,
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

/// Create the appropriate writer based on output format.
fn create_writer(
    format: OutputFormat,
    base_path: PathBuf,
    feed: String,
    stream_name: String,
    rotation_minutes: u32,
) -> Box<dyn ArchiveOutput> {
    match format {
        OutputFormat::Jsonl => Box::new(ArchiveWriter::new(
            base_path,
            feed,
            stream_name,
            rotation_minutes,
        )),
        OutputFormat::Parquet => Box::new(ParquetWriter::new(base_path, feed, stream_name)),
        OutputFormat::Both => Box::new(MultiWriter::new(
            base_path,
            feed,
            stream_name,
            rotation_minutes,
        )),
    }
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
    format: OutputFormat,
    shutdown: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stream_name = stream_config.name.clone();
    let rotation_minutes = (rotation_duration.as_secs() / 60) as u32;
    let format_str = format.to_string();

    info!(
        stream = %stream_config.stream,
        stream_name = %stream_name,
        format = %format_str,
        "Connecting to NATS"
    );

    // Connect to NATS
    let mut subscriber = Subscriber::connect(nats_url, &stream_config).await?;

    // Create writer based on configured format
    let mut writer = create_writer(
        format,
        base_path.to_path_buf(),
        feed.to_string(),
        stream_name.clone(),
        rotation_minutes,
    );

    // Track manifest data
    let mut tickers: HashSet<String> = HashSet::new();
    let mut message_types: HashSet<String> = HashSet::new();
    let mut gaps: Vec<Gap> = Vec::new();
    let mut completed_files: Vec<FileEntry> = Vec::new();
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
                    write_manifest(base_path, feed, &stream_name, &current_date, rotation_interval, &format_str, writer.as_mut(), &tickers, &message_types, &gaps, &mut completed_files)?;
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
                                warn!(stream_name = %stream_name, after_seq = after_seq, missing = missing, "Recording gap");
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

                            // Write to archive (returns rotated FileEntries on rotation)
                            let seq = msg.seq;
                            match writer.write(&msg.data, seq, now) {
                                Ok(rotated_entries) => {
                                    // Ack after successful write
                                    if let Err(e) = msg.ack().await {
                                        error!(stream_name = %stream_name, error = %e, seq = seq, "Failed to ack message");
                                    }
                                    for rotated_entry in rotated_entries {
                                        info!(
                                            stream_name = %stream_name,
                                            file = %rotated_entry.name,
                                            records = rotated_entry.records,
                                            seq_range = %format!("{}-{}", rotated_entry.nats_start_seq, rotated_entry.nats_end_seq),
                                            "File rotated"
                                        );
                                        completed_files.push(rotated_entry);
                                    }
                                    if !completed_files.is_empty() {
                                        if let Err(e) = update_manifest(base_path, feed, &stream_name, &current_date, rotation_interval, &format_str, &tickers, &message_types, &gaps, &completed_files) {
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
                        format = %format_str,
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
    info!(stream_name = %stream_name, "Writing final manifest");
    write_manifest(
        base_path,
        feed,
        &stream_name,
        &current_date,
        rotation_interval,
        &format_str,
        writer.as_mut(),
        &tickers,
        &message_types,
        &gaps,
        &mut completed_files,
    )?;

    info!(stream_name = %stream_name, "Archive task stopped");
    Ok(())
}

/// Update manifest with completed files (called on every rotation)
#[allow(clippy::too_many_arguments)]
fn update_manifest(
    base_path: &Path,
    feed: &str,
    stream_name: &str,
    date: &str,
    rotation_interval: &str,
    format: &str,
    tickers: &HashSet<String>,
    message_types: &HashSet<String>,
    gaps: &[Gap],
    completed_files: &[FileEntry],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut manifest = Manifest::new(feed, date, rotation_interval, format);
    manifest.files = completed_files.to_vec();
    manifest.tickers = tickers.iter().cloned().collect();
    manifest.message_types = message_types.iter().cloned().collect();
    manifest.gaps = gaps.to_vec();
    manifest.has_gaps = !gaps.is_empty();

    // Path: {base_path}/{feed}/{stream_name}/{date}/manifest.json
    let manifest_path = base_path
        .join(feed)
        .join(stream_name)
        .join(date)
        .join("manifest.json");
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, manifest_json)?;

    info!(stream_name = %stream_name, path = ?manifest_path, files = completed_files.len(), "Updated manifest");
    Ok(())
}

/// Write final manifest (called on shutdown/day rollover, closes current file)
#[allow(clippy::too_many_arguments)]
fn write_manifest(
    base_path: &Path,
    feed: &str,
    stream_name: &str,
    date: &str,
    rotation_interval: &str,
    format: &str,
    writer: &mut dyn ArchiveOutput,
    tickers: &HashSet<String>,
    message_types: &HashSet<String>,
    gaps: &[Gap],
    completed_files: &mut Vec<FileEntry>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Close current file and add to completed files
    completed_files.extend(writer.close()?);

    // Write manifest with all completed files
    update_manifest(
        base_path,
        feed,
        stream_name,
        date,
        rotation_interval,
        format,
        tickers,
        message_types,
        gaps,
        completed_files,
    )
}
