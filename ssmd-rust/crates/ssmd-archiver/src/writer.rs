use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::error::ArchiverError;
use crate::manifest::FileEntry;

/// Trait for archive output formats.
pub trait ArchiveOutput: Send {
    /// Write a message to the archive. Returns file entries from any rotated files.
    fn write(
        &mut self,
        data: &[u8],
        seq: u64,
        now: DateTime<Utc>,
    ) -> Result<Vec<FileEntry>, ArchiverError>;

    /// Close the writer and flush all remaining data. Returns file entries.
    fn close(&mut self) -> Result<Vec<FileEntry>, ArchiverError>;

    /// Flush buffered archive data to disk for durability before acking messages.
    fn flush(&mut self) -> Result<(), ArchiverError>;
}

/// Writes JSONL.gz files with rotation.
pub struct ArchiveWriter {
    base_path: PathBuf,
    feed: String,
    stream_name: String,
    current_file: Option<CurrentFile>,
    rotation_minutes: u32,
}

struct CurrentFile {
    path: PathBuf,
    final_name: String,
    encoder: GzEncoder<File>,
    start_time: DateTime<Utc>,
    records: u64,
    bytes_written: u64,
    first_seq: Option<u64>,
    last_seq: Option<u64>,
}

impl ArchiveWriter {
    pub fn new(
        base_path: PathBuf,
        feed: String,
        stream_name: String,
        rotation_minutes: u32,
    ) -> Self {
        Self {
            base_path,
            feed,
            stream_name,
            current_file: None,
            rotation_minutes,
        }
    }

    fn should_rotate(&self, now: DateTime<Utc>) -> bool {
        if let Some(ref file) = self.current_file {
            let elapsed = now.signed_duration_since(file.start_time);
            elapsed.num_minutes() >= self.rotation_minutes as i64
        } else {
            false
        }
    }

    fn rotate(&mut self, now: DateTime<Utc>) -> Result<Option<FileEntry>, ArchiverError> {
        if let Some(file) = self.current_file.take() {
            let entry = self.finish_file(file)?;
            self.open_new_file(now)?;
            return Ok(Some(entry));
        }
        Ok(None)
    }

    fn open_new_file(&mut self, now: DateTime<Utc>) -> Result<(), ArchiverError> {
        let date_str = now.format("%Y-%m-%d").to_string();
        let time_str = now.format("%H%M").to_string();

        // Path: {base_path}/{feed}/{stream_name}/{date}/
        let dir = self
            .base_path
            .join(&self.feed)
            .join(&self.stream_name)
            .join(&date_str);
        fs::create_dir_all(&dir)?;

        let mut filename = format!("{}.jsonl.gz", time_str);
        let mut path = dir.join(format!("{}.tmp", filename));
        let mut suffix: u32 = 1;

        while path.exists() || dir.join(&filename).exists() {
            filename = format!("{}-{:02}.jsonl.gz", time_str, suffix);
            path = dir.join(format!("{}.tmp", filename));
            suffix += 1;
        }

        let file = File::create(&path)?;
        let encoder = GzEncoder::new(file, Compression::default());

        self.current_file = Some(CurrentFile {
            path,
            final_name: filename,
            encoder,
            start_time: now,
            records: 0,
            bytes_written: 0,
            first_seq: None,
            last_seq: None,
        });

        Ok(())
    }

    fn finish_file(&self, file: CurrentFile) -> Result<FileEntry, ArchiverError> {
        file.encoder.finish()?;

        // Atomic rename from .tmp to final name
        let final_path = file.path.with_file_name(&file.final_name);
        fs::rename(&file.path, &final_path)?;

        let end_time = Utc::now();

        Ok(FileEntry {
            name: file.final_name,
            start: file.start_time,
            end: end_time,
            records: file.records,
            bytes: file.bytes_written,
            raw_bytes: None,
            compression_ratio: None,
            nats_start_seq: file.first_seq.unwrap_or(0),
            nats_end_seq: file.last_seq.unwrap_or(0),
            records_by_type: None,
        })
    }
}

impl ArchiveOutput for ArchiveWriter {
    fn write(
        &mut self,
        data: &[u8],
        seq: u64,
        now: DateTime<Utc>,
    ) -> Result<Vec<FileEntry>, ArchiverError> {
        // Check if we need to rotate
        let rotated = if self.should_rotate(now) {
            self.rotate(now)?
        } else {
            None
        };

        // Ensure we have a file open
        if self.current_file.is_none() {
            self.open_new_file(now)?;
        }

        let Some(file) = self.current_file.as_mut() else {
            return Err(ArchiverError::Io(std::io::Error::other(
                "archive writer missing active file",
            )));
        };

        // Track sequence
        if file.first_seq.is_none() {
            file.first_seq = Some(seq);
        }
        file.last_seq = Some(seq);

        // Inject _received_at and _nats_seq into JSON payload via byte-level
        // manipulation (no serde round-trip — this is the hot path).
        let received_at_micros = now.timestamp_micros();
        if let Some(pos) = data.iter().rposition(|&b| b == b'}') {
            file.encoder.write_all(&data[..pos])?;
            let suffix = format!(
                ",\"_received_at\":{},\"_nats_seq\":{}}}",
                received_at_micros, seq
            );
            file.encoder.write_all(suffix.as_bytes())?;
            file.encoder.write_all(b"\n")?;
            file.records += 1;
            file.bytes_written += pos as u64 + suffix.len() as u64 + 1;
        } else {
            // No closing brace — write raw (shouldn't happen for well-formed JSON)
            file.encoder.write_all(data)?;
            file.encoder.write_all(b"\n")?;
            file.records += 1;
            file.bytes_written += data.len() as u64 + 1;
        }

        Ok(rotated.into_iter().collect())
    }

    fn close(&mut self) -> Result<Vec<FileEntry>, ArchiverError> {
        if let Some(file) = self.current_file.take() {
            let entry = self.finish_file(file)?;
            return Ok(vec![entry]);
        }
        Ok(Vec::new())
    }

    fn flush(&mut self) -> Result<(), ArchiverError> {
        let Some(file) = self.current_file.as_mut() else {
            return Ok(());
        };

        // Flush gzip internal buffer to the OS page cache. We intentionally
        // skip fdatasync here — it runs every 100ms and the cost would hurt
        // throughput. Data reaches disk on rotation (finish_file) or OS writeback.
        file.encoder.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::{BufRead, BufReader};
    use tempfile::TempDir;

    /// Helper to read lines from a gzip file produced by ArchiveWriter.
    fn read_gz_lines(path: &std::path::Path) -> Vec<String> {
        let file = File::open(path).unwrap();
        let decoder = GzDecoder::new(file);
        let reader = BufReader::new(decoder);
        reader.lines().map(|l| l.unwrap()).collect()
    }

    #[test]
    fn test_write_records() {
        let tmp = TempDir::new().unwrap();
        let mut writer = ArchiveWriter::new(
            tmp.path().to_path_buf(),
            "kalshi".to_string(),
            "politics".to_string(),
            15,
        );

        let now = Utc::now();
        assert!(writer
            .write(br#"{"type":"trade","ticker":"INXD"}"#, 1, now)
            .unwrap()
            .is_empty());
        assert!(writer
            .write(br#"{"type":"trade","ticker":"KXBTC"}"#, 2, now)
            .unwrap()
            .is_empty());

        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.records, 2);
        assert_eq!(entry.nats_start_seq, 1);
        assert_eq!(entry.nats_end_seq, 2);

        // Verify final file exists and .tmp does not
        let date_str = now.format("%Y-%m-%d").to_string();
        let time_str = now.format("%H%M").to_string();
        let dir = tmp.path().join("kalshi").join("politics").join(&date_str);
        let final_path = dir.join(format!("{}.jsonl.gz", time_str));
        let tmp_path = dir.join(format!("{}.jsonl.gz.tmp", time_str));

        assert!(final_path.exists(), "final file should exist after close");
        assert!(!tmp_path.exists(), ".tmp file should not exist after close");

        let file = File::open(&final_path).unwrap();
        let decoder = GzDecoder::new(file);
        let reader = BufReader::new(decoder);
        let lines: Vec<_> = reader.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].as_ref().unwrap().contains("INXD"));
        assert!(lines[1].as_ref().unwrap().contains("KXBTC"));
    }

    #[test]
    fn test_tmp_file_during_write() {
        let tmp = TempDir::new().unwrap();
        let mut writer = ArchiveWriter::new(
            tmp.path().to_path_buf(),
            "kalshi".to_string(),
            "politics".to_string(),
            15,
        );

        let now = Utc::now();
        writer
            .write(br#"{"type":"trade","ticker":"INXD"}"#, 1, now)
            .unwrap();

        // During active write, .tmp should exist and final should not
        let date_str = now.format("%Y-%m-%d").to_string();
        let time_str = now.format("%H%M").to_string();
        let dir = tmp.path().join("kalshi").join("politics").join(&date_str);
        let tmp_path = dir.join(format!("{}.jsonl.gz.tmp", time_str));
        let final_path = dir.join(format!("{}.jsonl.gz", time_str));

        assert!(
            tmp_path.exists(),
            ".tmp file should exist during active write"
        );
        assert!(
            !final_path.exists(),
            "final file should not exist during active write"
        );

        // After close, .tmp gone and final exists
        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!tmp_path.exists(), ".tmp file should not exist after close");
        assert!(final_path.exists(), "final file should exist after close");
    }

    #[test]
    fn test_filename_collision_uses_suffix() {
        let tmp = TempDir::new().unwrap();
        let mut writer = ArchiveWriter::new(
            tmp.path().to_path_buf(),
            "kalshi".to_string(),
            "politics".to_string(),
            15,
        );

        let now = Utc::now();
        let date_str = now.format("%Y-%m-%d").to_string();
        let time_str = now.format("%H%M").to_string();
        let dir = tmp.path().join("kalshi").join("politics").join(&date_str);
        std::fs::create_dir_all(&dir).unwrap();

        let existing_final = dir.join(format!("{}.jsonl.gz", time_str));
        std::fs::write(&existing_final, b"existing").unwrap();

        writer
            .write(br#"{"type":"trade","ticker":"INXD"}"#, 1, now)
            .unwrap();
        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);

        assert_eq!(entries[0].name, format!("{}-01.jsonl.gz", time_str));
        assert!(dir.join(&entries[0].name).exists());
    }

    #[test]
    fn test_flush_succeeds_with_open_file() {
        let tmp = TempDir::new().unwrap();
        let mut writer = ArchiveWriter::new(
            tmp.path().to_path_buf(),
            "kalshi".to_string(),
            "politics".to_string(),
            15,
        );

        let now = Utc::now();
        writer
            .write(br#"{"type":"trade","ticker":"INXD"}"#, 1, now)
            .unwrap();

        writer.flush().unwrap();
        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].records, 1);
    }

    #[test]
    fn test_write_injects_metadata_fields() {
        let tmp = TempDir::new().unwrap();
        let mut writer = ArchiveWriter::new(
            tmp.path().to_path_buf(),
            "kalshi".to_string(),
            "politics".to_string(),
            15,
        );

        let now = Utc::now();
        writer
            .write(br#"{"type":"trade","ticker":"INXD"}"#, 42, now)
            .unwrap();
        writer
            .write(br#"{"type":"ticker","msg":{"market_ticker":"KXBTC"}}"#, 43, now)
            .unwrap();

        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);

        let date_str = now.format("%Y-%m-%d").to_string();
        let dir = tmp.path().join("kalshi").join("politics").join(&date_str);
        let final_path = dir.join(&entries[0].name);
        let lines = read_gz_lines(&final_path);

        assert_eq!(lines.len(), 2);

        // Verify first line has injected fields
        let json1: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(json1.get("_nats_seq").unwrap().as_u64().unwrap(), 42);
        assert_eq!(
            json1.get("_received_at").unwrap().as_i64().unwrap(),
            now.timestamp_micros()
        );
        // Original fields preserved
        assert_eq!(json1.get("type").unwrap().as_str().unwrap(), "trade");
        assert_eq!(json1.get("ticker").unwrap().as_str().unwrap(), "INXD");

        // Verify second line
        let json2: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(json2.get("_nats_seq").unwrap().as_u64().unwrap(), 43);
        assert_eq!(
            json2.get("_received_at").unwrap().as_i64().unwrap(),
            now.timestamp_micros()
        );
        // Nested content preserved
        assert_eq!(
            json2.get("msg").unwrap().get("market_ticker").unwrap().as_str().unwrap(),
            "KXBTC"
        );
    }

    #[test]
    fn test_bytes_written_accounts_for_injected_fields() {
        let tmp = TempDir::new().unwrap();
        let mut writer = ArchiveWriter::new(
            tmp.path().to_path_buf(),
            "kalshi".to_string(),
            "politics".to_string(),
            15,
        );

        let now = Utc::now();
        let data = br#"{"type":"trade"}"#;
        writer.write(data, 1, now).unwrap();

        let entries = writer.close().unwrap();
        // bytes_written should be greater than just data + newline
        // because we injected ,"_received_at":...,"_nats_seq":...}
        assert!(entries[0].bytes > data.len() as u64 + 1);
    }
}
