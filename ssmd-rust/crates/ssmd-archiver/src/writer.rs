use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::error::ArchiverError;
use crate::manifest::FileEntry;

/// Trait for archive output formats (JSONL.gz, Parquet, etc.).
pub trait ArchiveOutput: Send {
    /// Write a message to the archive. Returns file entries from any rotated files.
    fn write(&mut self, data: &[u8], seq: u64, now: DateTime<Utc>) -> Result<Vec<FileEntry>, ArchiverError>;

    /// Close the writer and flush all remaining data. Returns file entries.
    fn close(&mut self) -> Result<Vec<FileEntry>, ArchiverError>;
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
    pub fn new(base_path: PathBuf, feed: String, stream_name: String, rotation_minutes: u32) -> Self {
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
        let dir = self.base_path.join(&self.feed).join(&self.stream_name).join(&date_str);
        fs::create_dir_all(&dir)?;

        let filename = format!("{}.jsonl.gz", time_str);
        let tmp_filename = format!("{}.tmp", filename);
        let path = dir.join(&tmp_filename);

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
        })
    }
}

impl ArchiveOutput for ArchiveWriter {
    fn write(&mut self, data: &[u8], seq: u64, now: DateTime<Utc>) -> Result<Vec<FileEntry>, ArchiverError> {
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

        let file = self.current_file.as_mut().unwrap();

        // Track sequence
        if file.first_seq.is_none() {
            file.first_seq = Some(seq);
        }
        file.last_seq = Some(seq);

        // Write the line
        file.encoder.write_all(data)?;
        file.encoder.write_all(b"\n")?;
        file.records += 1;
        file.bytes_written += data.len() as u64 + 1;

        Ok(rotated.into_iter().collect())
    }

    fn close(&mut self) -> Result<Vec<FileEntry>, ArchiverError> {
        if let Some(file) = self.current_file.take() {
            let entry = self.finish_file(file)?;
            return Ok(vec![entry]);
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::{BufRead, BufReader};
    use tempfile::TempDir;

    #[test]
    fn test_write_records() {
        let tmp = TempDir::new().unwrap();
        let mut writer = ArchiveWriter::new(tmp.path().to_path_buf(), "kalshi".to_string(), "politics".to_string(), 15);

        let now = Utc::now();
        assert!(writer.write(br#"{"type":"trade","ticker":"INXD"}"#, 1, now).unwrap().is_empty());
        assert!(writer.write(br#"{"type":"trade","ticker":"KXBTC"}"#, 2, now).unwrap().is_empty());

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
        let mut writer = ArchiveWriter::new(tmp.path().to_path_buf(), "kalshi".to_string(), "politics".to_string(), 15);

        let now = Utc::now();
        writer.write(br#"{"type":"trade","ticker":"INXD"}"#, 1, now).unwrap();

        // During active write, .tmp should exist and final should not
        let date_str = now.format("%Y-%m-%d").to_string();
        let time_str = now.format("%H%M").to_string();
        let dir = tmp.path().join("kalshi").join("politics").join(&date_str);
        let tmp_path = dir.join(format!("{}.jsonl.gz.tmp", time_str));
        let final_path = dir.join(format!("{}.jsonl.gz", time_str));

        assert!(tmp_path.exists(), ".tmp file should exist during active write");
        assert!(!final_path.exists(), "final file should not exist during active write");

        // After close, .tmp gone and final exists
        let entries = writer.close().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!tmp_path.exists(), ".tmp file should not exist after close");
        assert!(final_path.exists(), "final file should exist after close");
    }
}
