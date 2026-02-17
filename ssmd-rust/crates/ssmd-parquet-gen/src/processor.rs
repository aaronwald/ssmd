use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use anyhow::{bail, Result};
use arrow::record_batch::RecordBatch;
use bytes::Bytes;
use chrono::{DateTime, NaiveDate, Utc};
use flate2::read::GzDecoder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::metadata::KeyValue;
use parquet::file::properties::{EnabledStatistics, WriterProperties};
use serde::Serialize;
use tracing::{info, warn};

use ssmd_schemas::{detect_message_type, MessageSchema, SchemaRegistry};

use crate::gcs::GcsClient;

fn for_each_gzip_line<F>(compressed: &[u8], mut on_line: F) -> std::io::Result<()>
where
    F: FnMut(&str),
{
    let decoder = GzDecoder::new(compressed);
    let mut reader = BufReader::new(decoder);
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf)?;
        if bytes_read == 0 {
            break;
        }

        let line = line_buf.trim_end_matches(['\n', '\r']);
        on_line(line);
    }

    Ok(())
}

/// Stats for a single hour's processing
#[derive(Debug, Default)]
pub struct HourStats {
    pub hour_key: String,
    pub files_read: usize,
    pub lines_total: usize,
    pub lines_empty: usize,
    pub lines_json_error: usize,
    pub lines_type_unknown: usize,
    pub lines_no_schema: HashMap<String, usize>,
    pub parse_batch_input: HashMap<String, usize>,
    pub parse_batch_dropped: HashMap<String, usize>,
    pub records_by_type: HashMap<String, usize>,
    pub parquet_files_written: usize,
    pub bytes_written: usize,
}

/// Stats for the parquet manifest (single hour or aggregated totals)
#[derive(Debug, Serialize, Default)]
pub struct ManifestStats {
    pub files_read: usize,
    pub lines_total: usize,
    pub lines_empty: usize,
    pub lines_json_error: usize,
    pub lines_type_unknown: usize,
    pub lines_no_schema: BTreeMap<String, usize>,
    pub parse_batch_input: BTreeMap<String, usize>,
    pub parse_batch_dropped: BTreeMap<String, usize>,
    pub records_written: BTreeMap<String, usize>,
}

/// Parquet generation manifest written to GCS
#[derive(Debug, Serialize)]
pub struct ParquetManifest {
    pub feed: String,
    pub stream: String,
    pub date: String,
    pub generated_at: String,
    pub version: String,
    pub hours: BTreeMap<String, ManifestStats>,
    pub totals: ManifestStats,
}

impl ManifestStats {
    fn from_hour_stats(stats: &HourStats) -> Self {
        Self {
            files_read: stats.files_read,
            lines_total: stats.lines_total,
            lines_empty: stats.lines_empty,
            lines_json_error: stats.lines_json_error,
            lines_type_unknown: stats.lines_type_unknown,
            lines_no_schema: stats.lines_no_schema.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            parse_batch_input: stats.parse_batch_input.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            parse_batch_dropped: stats.parse_batch_dropped.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            records_written: stats.records_by_type.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        }
    }

    fn merge(&mut self, other: &ManifestStats) {
        self.files_read += other.files_read;
        self.lines_total += other.lines_total;
        self.lines_empty += other.lines_empty;
        self.lines_json_error += other.lines_json_error;
        self.lines_type_unknown += other.lines_type_unknown;
        for (k, v) in &other.lines_no_schema {
            *self.lines_no_schema.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.parse_batch_input {
            *self.parse_batch_input.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.parse_batch_dropped {
            *self.parse_batch_dropped.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.records_written {
            *self.records_written.entry(k.clone()).or_default() += v;
        }
    }
}

/// Process all JSONL.gz files for a given feed/stream/date.
/// `gcs_prefix` is the top-level GCS prefix (matches archiver storage.remote.prefix).
/// Full GCS path: {gcs_prefix}/{feed}/{stream}/{date}/
pub async fn process_date(
    gcs: &GcsClient,
    gcs_prefix: &str,
    feed: &str,
    stream: &str,
    date: &NaiveDate,
    overwrite: bool,
    dry_run: bool,
) -> Result<Vec<HourStats>> {
    let registry = SchemaRegistry::for_feed(feed);
    let date_str = date.format("%Y-%m-%d").to_string();
    let prefix = format!("{}/{}/{}/{}", gcs_prefix, feed, stream, date_str);

    info!(prefix = %prefix, "Listing JSONL.gz files");
    let files = gcs.list_jsonl_files(&prefix).await?;

    if files.is_empty() {
        warn!(prefix = %prefix, "No JSONL.gz files found");
        return Ok(Vec::new());
    }

    info!(count = files.len(), "Found JSONL.gz files");

    // Group files by hour (extract HHMM from filename like "1415.jsonl.gz")
    let by_hour = group_files_by_hour(&files);

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
        let Some(hour_ts) = parse_hour_timestamp(date, hour_key) else {
            warn!(hour = %hour_key, "Invalid hour key, skipping hour group");
            continue;
        };

        let stats = process_hour(
            gcs,
            &registry,
            gcs_prefix,
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

    if !all_stats.is_empty() {
        write_manifest(gcs, gcs_prefix, feed, stream, &date_str, &all_stats).await?;
    }

    Ok(all_stats)
}

/// Process all files for a single hour
#[allow(clippy::too_many_arguments)]
async fn process_hour(
    gcs: &GcsClient,
    registry: &SchemaRegistry,
    gcs_prefix: &str,
    feed: &str,
    stream: &str,
    date_str: &str,
    hour_key: &str,
    files: &[String],
    hour_ts: DateTime<Utc>,
    overwrite: bool,
) -> Result<HourStats> {
    let mut stats = HourStats { hour_key: hour_key.to_string(), ..Default::default() };
    let hour_time_str = format!("{}00", hour_key);

    // Collect all messages from all files in this hour
    let mut messages_by_type: HashMap<String, Vec<(Vec<u8>, u64, i64)>> = HashMap::new();
    let mut line_counter: u64 = 0;
    // Fallback for old JSONL files that lack archiver-injected metadata
    let fallback_received_at = hour_ts.timestamp_micros();

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

        let line_result = for_each_gzip_line(&compressed, |line| {

            stats.lines_total += 1;

            if line.trim().is_empty() {
                stats.lines_empty += 1;
                return;
            }

            let json: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, "Failed to parse JSON line, skipping");
                    stats.lines_json_error += 1;
                    return;
                }
            };

            let msg_type = match detect_message_type(feed, &json) {
                Some(t) => t,
                None => {
                    stats.lines_type_unknown += 1;
                    return;
                }
            };

            // Check if we have a schema for this type
            if registry.get(&msg_type).is_none() {
                *stats.lines_no_schema.entry(msg_type).or_default() += 1;
                return;
            }

            line_counter += 1;

            // Extract archiver-injected metadata, or fall back for old files
            let msg_nats_seq = json.get("_nats_seq")
                .and_then(|v| v.as_u64())
                .unwrap_or(line_counter);
            let msg_received_at = json.get("_received_at")
                .and_then(|v| v.as_i64())
                .unwrap_or(fallback_received_at);

            messages_by_type
                .entry(msg_type)
                .or_default()
                .push((line.as_bytes().to_vec(), msg_nats_seq, msg_received_at));
        });

        if let Err(e) = line_result {
            warn!(file = %file_path, error = %e, "Failed to read decompressed line, skipping file");
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
            "{}/{}/{}/{}/{}_{}.parquet",
            gcs_prefix, feed, stream, date_str, msg_type, hour_time_str
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

        stats.parse_batch_input.insert(msg_type.clone(), messages.len());

        let batch = match schema.parse_batch(messages) {
            Ok(b) => b,
            Err(e) => {
                warn!(msg_type = %msg_type, error = %e, "Failed to parse batch, skipping");
                continue;
            }
        };

        if batch.num_rows() == 0 {
            bail!(
                "parse_batch returned 0 rows for type '{}' with {} input messages — likely schema field name mismatch",
                msg_type, messages.len()
            );
        }

        let dropped = messages.len() - batch.num_rows();
        if dropped > 0 {
            stats.parse_batch_dropped.insert(msg_type.clone(), dropped);
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
        lines_total = stats.lines_total,
        lines_empty = stats.lines_empty,
        lines_json_error = stats.lines_json_error,
        lines_type_unknown = stats.lines_type_unknown,
        parquet_files = stats.parquet_files_written,
        "Hour processing complete"
    );

    Ok(stats)
}

fn group_files_by_hour(files: &[String]) -> HashMap<String, Vec<String>> {
    let mut by_hour: HashMap<String, Vec<String>> = HashMap::new();

    for file_path in files {
        let filename = file_path.rsplit('/').next().unwrap_or(file_path);
        if let Some(hhmm) = filename.strip_suffix(".jsonl.gz") {
            if hhmm.len() >= 2 {
                let hour_key = &hhmm[..2];
                if let Ok(hour) = hour_key.parse::<u32>() {
                    if hour < 24 {
                        by_hour
                            .entry(hour_key.to_string())
                            .or_default()
                            .push(file_path.clone());
                    }
                }
            }
        }
    }

    by_hour
}

fn parse_hour_timestamp(date: &NaiveDate, hour_key: &str) -> Option<DateTime<Utc>> {
    let hour = hour_key.parse::<u32>().ok()?;
    if hour >= 24 {
        return None;
    }
    date.and_hms_opt(hour, 0, 0).map(|dt| dt.and_utc())
}

#[cfg(test)]
mod tests {
    use super::{group_files_by_hour, parse_hour_timestamp};
    use chrono::NaiveDate;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    use super::for_each_gzip_line;

    /// Extract archiver-injected metadata from a parsed JSON line.
    /// Returns (nats_seq, received_at_micros) using fallback values if absent.
    fn extract_metadata(json: &serde_json::Value, fallback_seq: u64, fallback_received_at: i64) -> (u64, i64) {
        let nats_seq = json.get("_nats_seq")
            .and_then(|v| v.as_u64())
            .unwrap_or(fallback_seq);
        let received_at = json.get("_received_at")
            .and_then(|v| v.as_i64())
            .unwrap_or(fallback_received_at);
        (nats_seq, received_at)
    }

    #[test]
    fn test_extract_metadata_with_injected_fields() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"trade","msg":{},"_received_at":1707667200123456,"_nats_seq":42}"#,
        ).unwrap();
        let (seq, recv_at) = extract_metadata(&json, 999, 0);
        assert_eq!(seq, 42);
        assert_eq!(recv_at, 1707667200123456);
    }

    #[test]
    fn test_extract_metadata_falls_back_for_old_files() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"trade","msg":{}}"#,
        ).unwrap();
        let (seq, recv_at) = extract_metadata(&json, 7, 1000000);
        assert_eq!(seq, 7);
        assert_eq!(recv_at, 1000000);
    }

    #[test]
    fn test_extract_metadata_partial_fields() {
        // Only _received_at present, _nats_seq absent
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"trade","_received_at":5555}"#,
        ).unwrap();
        let (seq, recv_at) = extract_metadata(&json, 10, 0);
        assert_eq!(seq, 10); // fallback
        assert_eq!(recv_at, 5555); // from JSON
    }

    #[test]
    fn test_group_files_by_hour_skips_invalid_hours() {
        let files = vec![
            "x/feed/stream/2026-02-14/0015.jsonl.gz".to_string(),
            "x/feed/stream/2026-02-14/2360.jsonl.gz".to_string(),
            "x/feed/stream/2026-02-14/2415.jsonl.gz".to_string(),
            "x/feed/stream/2026-02-14/ab15.jsonl.gz".to_string(),
            "x/feed/stream/2026-02-14/0015-01.jsonl.gz".to_string(),
        ];

        let grouped = group_files_by_hour(&files);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped.get("00").map(Vec::len), Some(2));
        assert_eq!(grouped.get("23").map(Vec::len), Some(1));
        assert!(grouped.get("24").is_none());
    }

    #[test]
    fn test_parse_hour_timestamp_bounds() {
        let date = NaiveDate::from_ymd_opt(2026, 2, 14).unwrap();

        assert!(parse_hour_timestamp(&date, "00").is_some());
        assert!(parse_hour_timestamp(&date, "23").is_some());
        assert!(parse_hour_timestamp(&date, "24").is_none());
        assert!(parse_hour_timestamp(&date, "xx").is_none());
    }

    #[test]
    fn test_streaming_reader_handles_large_line_count() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        for _ in 0..10_000 {
            encoder
                .write_all(br#"{"type":"ticker","msg":{"market_ticker":"KXBTC"}}"#)
                .unwrap();
            encoder
                .write_all(b"\n")
                .unwrap();
        }
        let compressed = encoder.finish().unwrap();

        let mut non_empty_lines = 0usize;
        for_each_gzip_line(&compressed, |line| {
            if !line.is_empty() {
                non_empty_lines += 1;
            }
        })
        .unwrap();

        assert_eq!(non_empty_lines, 10_000);
    }
}

/// Write parquet-manifest.json to GCS with per-hour and aggregate stats
async fn write_manifest(
    gcs: &GcsClient,
    gcs_prefix: &str,
    feed: &str,
    stream: &str,
    date_str: &str,
    all_stats: &[HourStats],
) -> Result<()> {
    let mut hours = BTreeMap::new();
    let mut totals = ManifestStats::default();

    for stats in all_stats {
        let hour_manifest = ManifestStats::from_hour_stats(stats);
        totals.merge(&hour_manifest);
        let hour_key_4digit = format!("{}00", stats.hour_key);
        hours.insert(hour_key_4digit, hour_manifest);
    }

    let manifest = ParquetManifest {
        feed: feed.to_string(),
        stream: stream.to_string(),
        date: date_str.to_string(),
        generated_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        version: "1.0.0".to_string(),
        hours,
        totals,
    };

    let json_bytes = serde_json::to_vec_pretty(&manifest)?;
    let manifest_path = format!(
        "{}/{}/{}/{}/parquet-manifest.json",
        gcs_prefix, feed, stream, date_str
    );

    info!(path = %manifest_path, bytes = json_bytes.len(), "Writing parquet manifest");
    gcs.put(&manifest_path, Bytes::from(json_bytes)).await?;

    Ok(())
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
