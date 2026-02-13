use std::collections::{HashMap, HashSet};
use std::io::Read;
use anyhow::Result;
use arrow::record_batch::RecordBatch;
use bytes::Bytes;
use chrono::{DateTime, NaiveDate, Utc};
use flate2::read::GzDecoder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::metadata::KeyValue;
use parquet::file::properties::{EnabledStatistics, WriterProperties};
use tracing::{info, warn};

use ssmd_schemas::{detect_message_type, MessageSchema, SchemaRegistry};

use crate::gcs::GcsClient;

/// Stats for a single hour's processing
#[derive(Debug, Default)]
pub struct HourStats {
    pub files_read: usize,
    pub lines_parsed: usize,
    pub lines_skipped: usize,
    pub dedup_count: u64,
    pub parquet_files_written: usize,
    pub records_by_type: HashMap<String, usize>,
    pub bytes_written: usize,
}

/// Process all JSONL.gz files for a given feed/stream/date
pub async fn process_date(
    gcs: &GcsClient,
    feed: &str,
    stream: &str,
    date: &NaiveDate,
    overwrite: bool,
    dry_run: bool,
) -> Result<Vec<HourStats>> {
    let registry = SchemaRegistry::for_feed(feed);
    let date_str = date.format("%Y-%m-%d").to_string();
    let prefix = format!("{}/{}/{}", feed, stream, date_str);

    info!(prefix = %prefix, "Listing JSONL.gz files");
    let files = gcs.list_jsonl_files(&prefix).await?;

    if files.is_empty() {
        warn!(prefix = %prefix, "No JSONL.gz files found");
        return Ok(Vec::new());
    }

    info!(count = files.len(), "Found JSONL.gz files");

    // Group files by hour (extract HHMM from filename like "1415.jsonl.gz")
    let mut by_hour: HashMap<String, Vec<String>> = HashMap::new();
    for file_path in &files {
        let filename = file_path.rsplit('/').next().unwrap_or(file_path);
        // Filename format: HHMM.jsonl.gz (e.g., "1415.jsonl.gz")
        if let Some(hhmm) = filename.strip_suffix(".jsonl.gz") {
            // Extract just the hour part (HH)
            if hhmm.len() >= 2 {
                let hour_key = &hhmm[..2];
                by_hour
                    .entry(hour_key.to_string())
                    .or_default()
                    .push(file_path.clone());
            }
        }
    }

    let mut hours: Vec<String> = by_hour.keys().cloned().collect();
    hours.sort();

    if dry_run {
        info!("Dry run — listing files by hour:");
        for hour in &hours {
            let hour_files = &by_hour[hour];
            info!(hour = %hour, files = hour_files.len(), "  Hour group");
            for f in hour_files {
                info!(file = %f, "    File");
            }
        }
        return Ok(Vec::new());
    }

    let mut all_stats = Vec::new();

    for hour_key in &hours {
        let hour_files = &by_hour[hour_key];
        let hour_num: u32 = hour_key.parse().unwrap_or(0);
        let hour_ts = date
            .and_hms_opt(hour_num, 0, 0)
            .map(|dt| dt.and_utc())
            .unwrap_or_else(|| date.and_hms_opt(0, 0, 0).unwrap().and_utc());

        let stats = process_hour(
            gcs,
            &registry,
            feed,
            stream,
            &date_str,
            hour_key,
            hour_files,
            hour_ts,
            overwrite,
        )
        .await?;

        all_stats.push(stats);
    }

    Ok(all_stats)
}

/// Process all files for a single hour
#[allow(clippy::too_many_arguments)]
async fn process_hour(
    gcs: &GcsClient,
    registry: &SchemaRegistry,
    feed: &str,
    stream: &str,
    date_str: &str,
    hour_key: &str,
    files: &[String],
    hour_ts: DateTime<Utc>,
    overwrite: bool,
) -> Result<HourStats> {
    let mut stats = HourStats::default();
    let hour_time_str = format!("{}00", hour_key);

    // Collect all messages from all files in this hour
    let mut messages_by_type: HashMap<String, Vec<(Vec<u8>, u64, i64)>> = HashMap::new();
    let mut dedup_set: HashSet<u64> = HashSet::new();
    let mut line_counter: u64 = 0;
    let received_at_micros = hour_ts.timestamp_micros();

    for file_path in files {
        info!(file = %file_path, "Downloading JSONL.gz");
        let compressed = match gcs.get(file_path).await {
            Ok(data) => data,
            Err(e) => {
                warn!(file = %file_path, error = %e, "Failed to download, skipping");
                continue;
            }
        };

        stats.files_read += 1;

        let mut decoder = GzDecoder::new(&compressed[..]);
        let mut content = String::new();
        if let Err(e) = decoder.read_to_string(&mut content) {
            warn!(file = %file_path, error = %e, "Failed to decompress, skipping");
            continue;
        }

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let json: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, "Failed to parse JSON line, skipping");
                    stats.lines_skipped += 1;
                    continue;
                }
            };

            let msg_type = match detect_message_type(feed, &json) {
                Some(t) => t,
                None => {
                    stats.lines_skipped += 1;
                    continue;
                }
            };

            // Check if we have a schema for this type
            let dedup_key = match registry.get(&msg_type) {
                Some(schema) => schema.dedup_key(&json),
                None => {
                    // No schema registered for this type — skip silently
                    continue;
                }
            };

            // Dedup check
            if let Some(key) = dedup_key {
                if !dedup_set.insert(key) {
                    stats.dedup_count += 1;
                    continue;
                }
            }

            line_counter += 1;
            stats.lines_parsed += 1;

            messages_by_type
                .entry(msg_type)
                .or_default()
                .push((line.as_bytes().to_vec(), line_counter, received_at_micros));
        }
    }

    // Write parquet for each message type
    for (msg_type, messages) in &messages_by_type {
        let schema = match registry.get(msg_type) {
            Some(s) => s,
            None => continue,
        };

        // Check if parquet already exists
        let parquet_path = format!(
            "{}/{}/{}/{}_{}.parquet",
            feed, stream, date_str, msg_type, hour_time_str
        );

        if !overwrite {
            match gcs.exists(&parquet_path).await {
                Ok(true) => {
                    info!(path = %parquet_path, "Parquet exists, skipping (use --overwrite to replace)");
                    continue;
                }
                Ok(false) => {}
                Err(e) => {
                    warn!(path = %parquet_path, error = %e, "Failed to check existence, proceeding");
                }
            }
        }

        let batch = match schema.parse_batch(messages) {
            Ok(b) => b,
            Err(e) => {
                warn!(msg_type = %msg_type, error = %e, "Failed to parse batch, skipping");
                continue;
            }
        };

        if batch.num_rows() == 0 {
            warn!(msg_type = %msg_type, messages = messages.len(), "parse_batch returned 0 rows, skipping");
            continue;
        }

        let parquet_bytes = write_parquet_to_bytes(&batch, schema)?;
        let bytes_len = parquet_bytes.len();

        gcs.put(&parquet_path, Bytes::from(parquet_bytes)).await?;

        info!(
            path = %parquet_path,
            records = batch.num_rows(),
            bytes = bytes_len,
            "Wrote parquet file"
        );

        stats.parquet_files_written += 1;
        stats
            .records_by_type
            .insert(msg_type.clone(), batch.num_rows());
        stats.bytes_written += bytes_len;
    }

    info!(
        hour = %hour_key,
        files_read = stats.files_read,
        lines_parsed = stats.lines_parsed,
        lines_skipped = stats.lines_skipped,
        dedup_count = stats.dedup_count,
        parquet_files = stats.parquet_files_written,
        "Hour processing complete"
    );

    Ok(stats)
}

/// Write a RecordBatch to Parquet bytes in memory
fn write_parquet_to_bytes(batch: &RecordBatch, schema: &dyn MessageSchema) -> Result<Vec<u8>> {
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_max_row_group_size(100_000)
        .set_data_page_size_limit(1024 * 1024) // 1MB
        .set_statistics_enabled(EnabledStatistics::Chunk)
        .set_created_by("ssmd-parquet-gen".to_string())
        .set_key_value_metadata(Some(vec![
            KeyValue::new(
                "ssmd.schema_name".to_string(),
                schema.schema_name().to_string(),
            ),
            KeyValue::new(
                "ssmd.schema_version".to_string(),
                schema.schema_version().to_string(),
            ),
        ]))
        .build();

    let mut buf = Vec::new();
    let mut writer = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props))?;
    writer.write(batch)?;
    writer.close()?;
    Ok(buf)
}
