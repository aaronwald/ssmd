use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::error::ArchiverError;
use crate::manifest::FileEntry;

/// Writes JSONL.gz files with rotation
pub struct ArchiveWriter {
    base_path: PathBuf,
    feed: String,
    current_file: Option<CurrentFile>,
    rotation_minutes: u32,
}

struct CurrentFile {
    path: PathBuf,
    encoder: GzEncoder<File>,
    start_time: DateTime<Utc>,
    records: u64,
    bytes_written: u64,
    first_seq: Option<u64>,
    last_seq: Option<u64>,
}

impl ArchiveWriter {
    pub fn new(base_path: PathBuf, feed: String, rotation_minutes: u32) -> Self {
        Self {
            base_path,
            feed,
            current_file: None,
            rotation_minutes,
        }
    }

    /// Write a record to the current file, rotating if needed
    pub fn write(&mut self, data: &[u8], seq: u64, now: DateTime<Utc>) -> Result<(), ArchiverError> {
        // Check if we need to rotate
        if self.should_rotate(now) {
            self.rotate(now)?;
        }

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

        Ok(())
    }

    /// Flush and close current file, returning FileEntry for manifest
    pub fn close(&mut self) -> Result<Option<FileEntry>, ArchiverError> {
        if let Some(file) = self.current_file.take() {
            let entry = self.finish_file(file)?;
            return Ok(Some(entry));
        }
        Ok(None)
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

        let dir = self.base_path.join(&self.feed).join(&date_str);
        fs::create_dir_all(&dir)?;

        let filename = format!("{}.jsonl.gz", time_str);
        let path = dir.join(&filename);

        let file = File::create(&path)?;
        let encoder = GzEncoder::new(file, Compression::default());

        self.current_file = Some(CurrentFile {
            path,
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

        let end_time = Utc::now();

        Ok(FileEntry {
            name: file.path.file_name().unwrap().to_string_lossy().to_string(),
            start: file.start_time,
            end: end_time,
            records: file.records,
            bytes: file.bytes_written,
            nats_start_seq: file.first_seq.unwrap_or(0),
            nats_end_seq: file.last_seq.unwrap_or(0),
        })
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
        let mut writer = ArchiveWriter::new(tmp.path().to_path_buf(), "kalshi".to_string(), 15);

        let now = Utc::now();
        writer.write(br#"{"type":"trade","ticker":"INXD"}"#, 1, now).unwrap();
        writer.write(br#"{"type":"trade","ticker":"KXBTC"}"#, 2, now).unwrap();

        let entry = writer.close().unwrap().unwrap();
        assert_eq!(entry.records, 2);
        assert_eq!(entry.nats_start_seq, 1);
        assert_eq!(entry.nats_end_seq, 2);

        // Verify file contents
        let date_str = now.format("%Y-%m-%d").to_string();
        let time_str = now.format("%H%M").to_string();
        let path = tmp.path().join("kalshi").join(&date_str).join(format!("{}.jsonl.gz", time_str));

        let file = File::open(&path).unwrap();
        let decoder = GzDecoder::new(file);
        let reader = BufReader::new(decoder);
        let lines: Vec<_> = reader.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].as_ref().unwrap().contains("INXD"));
        assert!(lines[1].as_ref().unwrap().contains("KXBTC"));
    }
}
