use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Timelike, Utc};
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::{EnabledStatistics, WriterProperties};
use parquet::file::metadata::KeyValue;
use tracing::{error, info, warn};

use crate::error::ArchiverError;
use crate::manifest::FileEntry;
use crate::schema::{self, MessageSchema, SchemaRegistry};
use crate::writer::ArchiveOutput;

/// Buffer for accumulating raw messages of a single type.
struct MessageBuffer {
    messages: Vec<(Vec<u8>, u64, i64)>,
    bytes_estimate: usize,
    first_seq: Option<u64>,
    last_seq: Option<u64>,
}

impl MessageBuffer {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            bytes_estimate: 0,
            first_seq: None,
            last_seq: None,
        }
    }

    fn add(&mut self, data: Vec<u8>, seq: u64, received_at_micros: i64) {
        self.bytes_estimate += data.len();
        if self.first_seq.is_none() {
            self.first_seq = Some(seq);
        }
        self.last_seq = Some(seq);
        self.messages.push((data, seq, received_at_micros));
    }
}

/// Writes Parquet files with hourly rotation and deduplication.
pub struct ParquetWriter {
    base_path: PathBuf,
    feed: String,
    stream_name: String,
    registry: SchemaRegistry,
    buffers: HashMap<String, MessageBuffer>,
    dedup_set: HashSet<u64>,
    dedup_count: u64,
    current_hour: Option<DateTime<Utc>>,
}

impl ParquetWriter {
    pub fn new(base_path: PathBuf, feed: String, stream_name: String) -> Self {
        let registry = SchemaRegistry::for_feed(&feed);
        Self {
            base_path,
            feed,
            stream_name,
            registry,
            buffers: HashMap::new(),
            dedup_set: HashSet::new(),
            dedup_count: 0,
            current_hour: None,
        }
    }

    /// Number of duplicate messages skipped in the current hour.
    pub fn dedup_count(&self) -> u64 {
        self.dedup_count
    }

    /// Flush all message-type buffers to parquet files.
    fn flush_all(&mut self, hour: DateTime<Utc>) -> Result<Vec<FileEntry>, ArchiverError> {
        let date_str = hour.format("%Y-%m-%d").to_string();
        let time_str = hour.format("%H%M").to_string();
        let dir = self
            .base_path
            .join(&self.feed)
            .join(&self.stream_name)
            .join(&date_str);
        fs::create_dir_all(&dir)?;

        // Drain buffers into a local vec to avoid borrow conflicts with self.registry
        let drained: Vec<(String, MessageBuffer)> = self.buffers.drain().collect();
        let mut entries = Vec::new();

        let mut schema_errors: Vec<String> = Vec::new();

        for (msg_type, buffer) in &drained {
            if buffer.messages.is_empty() {
                continue;
            }

            let schema = match self.registry.get(msg_type) {
                Some(s) => s,
                None => continue,
            };

            match write_parquet_file(schema, msg_type, buffer, &dir, &time_str, hour) {
                Ok(Some(entry)) => {
                    info!(
                        feed = %self.feed,
                        stream = %self.stream_name,
                        file = %entry.name,
                        records = entry.records,
                        bytes = entry.bytes,
                        raw_bytes = ?entry.raw_bytes,
                        compression_ratio = ?entry.compression_ratio,
                        "Flushed parquet file"
                    );
                    entries.push(entry);
                }
                Ok(None) => {}
                Err(ArchiverError::SchemaValidation(msg)) => {
                    error!(
                        feed = %self.feed,
                        stream = %self.stream_name,
                        msg_type = %msg_type,
                        "{msg}"
                    );
                    schema_errors.push(msg);
                }
                Err(e) => {
                    warn!(
                        msg_type = %msg_type,
                        error = %e,
                        "Failed to write parquet file, skipping"
                    );
                }
            }
        }

        // Schema validation errors are fatal — archiver should crash-restart
        // so the issue surfaces via CrashLoopBackOff alerts.
        if let Some(first_error) = schema_errors.into_iter().next() {
            return Err(ArchiverError::SchemaValidation(first_error));
        }

        Ok(entries)
    }
}

impl ArchiveOutput for ParquetWriter {
    fn write(
        &mut self,
        data: &[u8],
        seq: u64,
        now: DateTime<Utc>,
    ) -> Result<Vec<FileEntry>, ArchiverError> {
        let hour = truncate_to_hour(now);

        // Check for hour rotation
        let mut rotated_files = Vec::new();
        if let Some(current) = self.current_hour {
            if hour != current {
                rotated_files = self.flush_all(current)?;
                self.dedup_set.clear();
                self.dedup_count = 0;
            }
        }
        self.current_hour = Some(hour);

        // Parse JSON to detect message type
        let json: serde_json::Value =
            serde_json::from_slice(data).map_err(ArchiverError::Serialization)?;

        let msg_type = match schema::detect_message_type(&self.feed, &json) {
            Some(t) => t,
            None => return Ok(rotated_files),
        };

        // Check if we have a schema for this type (skip unrecognized types)
        let dedup_key = match self.registry.get(&msg_type) {
            Some(schema) => schema.dedup_key(&json),
            None => return Ok(rotated_files),
        };

        // Dedup check
        if let Some(key) = dedup_key {
            if !self.dedup_set.insert(key) {
                self.dedup_count += 1;
                return Ok(rotated_files);
            }
        }

        // Buffer the message
        let received_at = now.timestamp_micros();
        let buffer = self
            .buffers
            .entry(msg_type)
            .or_insert_with(MessageBuffer::new);
        buffer.add(data.to_vec(), seq, received_at);

        Ok(rotated_files)
    }

    fn close(&mut self) -> Result<Vec<FileEntry>, ArchiverError> {
        if let Some(hour) = self.current_hour.take() {
            self.flush_all(hour)
        } else {
            Ok(Vec::new())
        }
    }
}

/// Truncate a DateTime to the hour boundary.
fn truncate_to_hour(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(dt)
}

/// Write a single parquet file from a message buffer.
fn write_parquet_file(
    schema: &dyn MessageSchema,
    msg_type: &str,
    buffer: &MessageBuffer,
    dir: &Path,
    time_str: &str,
    hour: DateTime<Utc>,
) -> Result<Option<FileEntry>, ArchiverError> {
    let batch = schema
        .parse_batch(&buffer.messages)
        .map_err(ArchiverError::Arrow)?;

    if batch.num_rows() == 0 {
        if !buffer.messages.is_empty() {
            // All messages failed to parse — this is a schema mismatch, not an empty stream.
            // Fail fast: this means the archiver's Arrow schema does not match the raw WS JSON.
            return Err(ArchiverError::SchemaValidation(format!(
                "All {} '{}' messages failed to parse (0 rows produced). \
                 Schema mismatch suspected — check field names against raw WebSocket JSON.",
                buffer.messages.len(),
                msg_type
            )));
        }
        return Ok(None);
    }

    let filename = format!("{}_{}.parquet", msg_type, time_str);
    let tmp_filename = format!("{}.tmp", filename);
    let tmp_path = dir.join(&tmp_filename);
    let final_path = dir.join(&filename);

    let file = File::create(&tmp_path)?;
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_max_row_group_size(100_000)
        .set_data_page_size_limit(1024 * 1024) // 1MB
        .set_statistics_enabled(EnabledStatistics::Chunk)
        .set_created_by("ssmd-archiver".to_string())
        .set_key_value_metadata(Some(vec![
            KeyValue::new("ssmd.schema_name".to_string(), schema.schema_name().to_string()),
            KeyValue::new("ssmd.schema_version".to_string(), schema.schema_version().to_string()),
        ]))
        .build();

    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    // Atomic rename from .tmp to final name
    fs::rename(&tmp_path, &final_path)?;

    let file_size = std::fs::metadata(&final_path)?.len();
    let raw_bytes = buffer.bytes_estimate as u64;
    let compression_ratio = if file_size > 0 {
        Some(raw_bytes as f64 / file_size as f64)
    } else {
        None
    };

    Ok(Some(FileEntry {
        name: filename,
        start: hour,
        end: hour + chrono::Duration::hours(1),
        records: batch.num_rows() as u64,
        bytes: file_size,
        raw_bytes: Some(raw_bytes),
        compression_ratio,
        nats_start_seq: buffer.first_seq.unwrap_or(0),
        nats_end_seq: buffer.last_seq.unwrap_or(0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::*;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use tempfile::TempDir;

    fn make_kalshi_ticker(ticker: &str, ts: i64) -> Vec<u8> {
        format!(
            r#"{{"type":"ticker","sid":1,"msg":{{"market_ticker":"{}","yes_bid":50,"yes_ask":52,"price":51,"volume":1000,"ts":{},"Clock":13281241747}}}}"#,
            ticker, ts
        )
        .into_bytes()
    }

    fn make_kalshi_trade(ticker: &str, price: i64, side: &str, ts: i64) -> Vec<u8> {
        // Use real Kalshi WS field names: yes_price, taker_side
        // Include trade_id (required) and envelope seq (nullable)
        format!(
            r#"{{"type":"trade","sid":1,"seq":{},"msg":{{"trade_id":"tid-{}-{}","market_ticker":"{}","yes_price":{},"count":10,"taker_side":"{}","ts":{}}}}}"#,
            ts, ticker, ts, ticker, price, side, ts
        )
        .into_bytes()
    }

    fn make_writer(tmp: &TempDir) -> ParquetWriter {
        ParquetWriter::new(
            tmp.path().to_path_buf(),
            "kalshi".to_string(),
            "crypto".to_string(),
        )
    }

    fn hour(year: i32, month: u32, day: u32, h: u32) -> DateTime<Utc> {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn test_buffer_and_flush() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 2, 12, 14);
        let msg1 = make_kalshi_ticker("KXBTC-1", 1707667200);
        let msg2 = make_kalshi_ticker("KXBTC-2", 1707667201);

        assert!(writer.write(&msg1, 1, now).unwrap().is_empty());
        assert!(writer.write(&msg2, 2, now).unwrap().is_empty());

        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.name, "ticker_1400.parquet");
        assert_eq!(entry.records, 2);
        assert_eq!(entry.nats_start_seq, 1);
        assert_eq!(entry.nats_end_seq, 2);
        assert!(entry.bytes > 0);
        assert!(entry.raw_bytes.unwrap() > 0);
        assert!(entry.compression_ratio.unwrap() > 0.0);

        // Read back and verify
        let path = tmp
            .path()
            .join("kalshi")
            .join("crypto")
            .join("2026-02-12")
            .join("ticker_1400.parquet");
        let file = File::open(&path).unwrap();
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap();

        let batches: Vec<_> = reader.collect::<Result<_, _>>().unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows(), 2);

        let tickers = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(tickers.value(0), "KXBTC-1");
        assert_eq!(tickers.value(1), "KXBTC-2");
    }

    #[test]
    fn test_multiple_message_types() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 2, 12, 10);
        let ticker = make_kalshi_ticker("KXBTC", 1707667200);
        let trade = make_kalshi_trade("KXBTC", 55, "yes", 1707667201);

        writer.write(&ticker, 1, now).unwrap();
        writer.write(&trade, 2, now).unwrap();

        let entries = writer.close().unwrap();
        // Should produce two files: ticker and trade
        assert_eq!(entries.len(), 2);

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"ticker_1000.parquet"));
        assert!(names.contains(&"trade_1000.parquet"));
    }

    #[test]
    fn test_hourly_rotation() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let hour1 = hour(2026, 2, 12, 14);
        let hour2 = hour(2026, 2, 12, 15);

        let msg1 = make_kalshi_ticker("KXBTC", 1707667200);
        let msg2 = make_kalshi_ticker("KXBTC", 1707667201);

        // Write in hour 1
        assert!(writer.write(&msg1, 1, hour1).unwrap().is_empty());

        // Write in hour 2 → triggers rotation, returns hour 1 files
        let rotated = writer.write(&msg2, 2, hour2).unwrap();
        assert_eq!(rotated.len(), 1);
        assert_eq!(rotated[0].name, "ticker_1400.parquet");
        assert_eq!(rotated[0].records, 1);

        // Close flushes hour 2
        let final_entries = writer.close().unwrap();
        assert_eq!(final_entries.len(), 1);
        assert_eq!(final_entries[0].name, "ticker_1500.parquet");
    }

    #[test]
    fn test_deduplication() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 2, 12, 14);
        let msg = make_kalshi_ticker("KXBTC", 1707667200);

        // Write same message twice
        writer.write(&msg, 1, now).unwrap();
        writer.write(&msg, 2, now).unwrap();

        assert_eq!(writer.dedup_count(), 1);

        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].records, 1); // Only one row, duplicate was skipped
    }

    #[test]
    fn test_dedup_clears_on_rotation() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let hour1 = hour(2026, 2, 12, 14);
        let hour2 = hour(2026, 2, 12, 15);
        let msg = make_kalshi_ticker("KXBTC", 1707667200);

        // Write in hour 1
        writer.write(&msg, 1, hour1).unwrap();
        // Duplicate in hour 1 → skipped
        writer.write(&msg, 2, hour1).unwrap();
        assert_eq!(writer.dedup_count(), 1);

        // Move to hour 2 → dedup set clears, same message accepted again
        writer.write(&msg, 3, hour2).unwrap();
        assert_eq!(writer.dedup_count(), 0); // Reset after rotation

        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].records, 1);
    }

    #[test]
    fn test_skip_non_data_messages() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 2, 12, 14);
        let subscribed = br#"{"type":"subscribed","msg":{}}"#;
        let ok_msg = br#"{"type":"ok","sid":1}"#;

        writer.write(subscribed, 1, now).unwrap();
        writer.write(ok_msg, 2, now).unwrap();

        let entries = writer.close().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_compression_ratio() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 2, 12, 14);
        // Write several messages to get meaningful compression
        for i in 0..20 {
            let msg = make_kalshi_ticker(&format!("KXBTC-{}", i), 1707667200 + i);
            writer.write(&msg, i as u64, now).unwrap();
        }

        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert!(entry.raw_bytes.is_some());
        assert!(entry.compression_ratio.is_some());
        let ratio = entry.compression_ratio.unwrap();
        // Parquet with snappy should achieve some compression
        assert!(ratio > 0.0, "compression ratio should be positive");
    }

    #[test]
    fn test_file_naming() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 1, 15, 4);
        let msg = make_kalshi_trade("KXBTC", 50, "yes", 1707667200);
        writer.write(&msg, 1, now).unwrap();

        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "trade_0400.parquet");

        // Verify the file exists at the expected path
        let path = tmp
            .path()
            .join("kalshi")
            .join("crypto")
            .join("2026-01-15")
            .join("trade_0400.parquet");
        assert!(path.exists());
    }

    #[test]
    fn test_empty_close() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let entries = writer.close().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parquet_readback_trade() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 2, 12, 10);
        let trade = make_kalshi_trade("KXBTC-123", 55, "yes", 1707667200);
        writer.write(&trade, 42, now).unwrap();

        let entries = writer.close().unwrap();
        let path = tmp
            .path()
            .join("kalshi")
            .join("crypto")
            .join("2026-02-12")
            .join(&entries[0].name);

        let file = File::open(&path).unwrap();
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap();
        let batches: Vec<_> = reader.collect::<Result<_, _>>().unwrap();
        assert_eq!(batches[0].num_rows(), 1);

        let tickers = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(tickers.value(0), "KXBTC-123");

        let prices = batches[0]
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(prices.value(0), 55);

        let sides = batches[0]
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(sides.value(0), "yes");
    }

    #[test]
    fn test_no_tmp_files_after_close() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 2, 12, 14);
        let ticker = make_kalshi_ticker("KXBTC", 1707667200);
        let trade = make_kalshi_trade("KXBTC", 55, "yes", 1707667201);

        writer.write(&ticker, 1, now).unwrap();
        writer.write(&trade, 2, now).unwrap();

        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 2);

        // No .tmp files should remain in the output directory
        let dir = tmp.path().join("kalshi").join("crypto").join("2026-02-12");
        let tmp_files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "tmp"))
            .collect();
        assert!(tmp_files.is_empty(), "no .tmp files should remain after close, found: {:?}", tmp_files);

        // Final files should exist
        assert!(dir.join("ticker_1400.parquet").exists());
        assert!(dir.join("trade_1400.parquet").exists());
    }

    #[test]
    fn test_schema_validation_error_on_total_parse_failure() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 2, 12, 14);
        // Trade message with completely wrong field names — no yes_price/price, no taker_side/side
        let bad_trade = br#"{"type":"trade","sid":1,"seq":1,"msg":{"trade_id":"tid-bad","market_ticker":"KXBTC","wrong_field":55,"count":10,"bad_side":"yes","ts":100}}"#;

        writer.write(bad_trade, 1, now).unwrap();

        // Close should fail with SchemaValidation error because all trade messages failed
        let result = writer.close();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Schema validation error"),
            "Expected SchemaValidation error, got: {}",
            err
        );
    }

    #[test]
    fn test_parquet_schema_metadata() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let now = hour(2026, 2, 12, 14);
        let ticker = make_kalshi_ticker("KXBTC", 1707667200);
        let trade = make_kalshi_trade("KXBTC", 55, "yes", 1707667201);

        writer.write(&ticker, 1, now).unwrap();
        writer.write(&trade, 2, now).unwrap();
        writer.close().unwrap();

        let dir = tmp.path().join("kalshi/crypto/2026-02-12");

        // Check ticker metadata
        let ticker_file = File::open(dir.join("ticker_1400.parquet")).unwrap();
        let ticker_reader = ParquetRecordBatchReaderBuilder::try_new(ticker_file).unwrap();
        let ticker_meta = ticker_reader.metadata().file_metadata().key_value_metadata().unwrap();
        let ticker_kv: std::collections::HashMap<&str, &str> = ticker_meta
            .iter()
            .filter_map(|kv| Some((kv.key.as_str(), kv.value.as_ref()?.as_str())))
            .collect();
        assert_eq!(ticker_kv.get("ssmd.schema_name"), Some(&"kalshi_ticker"));
        assert_eq!(ticker_kv.get("ssmd.schema_version"), Some(&"1.1.0"));

        // Check trade metadata
        let trade_file = File::open(dir.join("trade_1400.parquet")).unwrap();
        let trade_reader = ParquetRecordBatchReaderBuilder::try_new(trade_file).unwrap();
        let trade_meta = trade_reader.metadata().file_metadata().key_value_metadata().unwrap();
        let trade_kv: std::collections::HashMap<&str, &str> = trade_meta
            .iter()
            .filter_map(|kv| Some((kv.key.as_str(), kv.value.as_ref()?.as_str())))
            .collect();
        assert_eq!(trade_kv.get("ssmd.schema_name"), Some(&"kalshi_trade"));
        assert_eq!(trade_kv.get("ssmd.schema_version"), Some(&"1.1.0"));
    }

    #[test]
    fn test_day_boundary_rotation() {
        let tmp = TempDir::new().unwrap();
        let mut writer = make_writer(&tmp);

        let day1 = hour(2026, 2, 12, 23);
        let day2 = hour(2026, 2, 13, 0);

        let msg1 = make_kalshi_ticker("KXBTC", 1707667200);
        let msg2 = make_kalshi_ticker("KXBTC", 1707667201);

        writer.write(&msg1, 1, day1).unwrap();
        let rotated = writer.write(&msg2, 2, day2).unwrap();

        // Day 1 file rotated
        assert_eq!(rotated.len(), 1);
        assert_eq!(rotated[0].name, "ticker_2300.parquet");

        let final_entries = writer.close().unwrap();
        assert_eq!(final_entries.len(), 1);
        assert_eq!(final_entries[0].name, "ticker_0000.parquet");

        // Verify files in correct date directories
        assert!(tmp
            .path()
            .join("kalshi/crypto/2026-02-12/ticker_2300.parquet")
            .exists());
        assert!(tmp
            .path()
            .join("kalshi/crypto/2026-02-13/ticker_0000.parquet")
            .exists());
    }
}
