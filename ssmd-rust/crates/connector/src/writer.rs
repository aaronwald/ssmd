use async_trait::async_trait;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::WriterError;
use crate::message::Message;
use crate::traits::Writer;

/// Writes messages to date-partitioned JSONL files.
/// Wall-clock timestamps applied here (syscall OK - we're doing disk I/O anyway).
pub struct FileWriter {
    base_dir: PathBuf,
    feed_name: String,
    inner: Mutex<FileWriterInner>,
}

struct FileWriterInner {
    writer: Option<BufWriter<File>>,
    current_date: String,
}

impl FileWriter {
    pub fn new(base_dir: impl Into<PathBuf>, feed_name: impl Into<String>) -> Self {
        Self {
            base_dir: base_dir.into(),
            feed_name: feed_name.into(),
            inner: Mutex::new(FileWriterInner {
                writer: None,
                current_date: String::new(),
            }),
        }
    }
}

#[async_trait]
impl Writer for FileWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        // Wall-clock timestamp at I/O boundary (syscall OK here)
        let now = chrono::Utc::now();
        let date = now.format("%Y-%m-%d").to_string();

        let mut inner = self.inner.lock().unwrap();

        // Rotate file if date changed
        if date != inner.current_date {
            if let Some(ref mut writer) = inner.writer {
                writer.flush()?;
            }

            let dir = self.base_dir.join(&date);
            fs::create_dir_all(&dir)?;

            let path = dir.join(format!("{}.jsonl", self.feed_name));
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;

            inner.writer = Some(BufWriter::new(file));
            inner.current_date = date;
        }

        // Write JSONL: {"ts":"...", "feed":"...", "data": <raw bytes>}
        // Raw bytes written directly - no JSON parsing/re-serialization
        if let Some(ref mut writer) = inner.writer {
            let ts = now.to_rfc3339();
            write!(writer, "{{\"ts\":\"{}\",\"feed\":\"{}\",\"data\":", ts, msg.feed)?;
            writer.write_all(&msg.data)?;
            writeln!(writer, "}}")?;
        }

        Ok(())
    }

    async fn close(&mut self) -> Result<(), WriterError> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(ref mut writer) = inner.writer {
            writer.flush()?;
        }
        inner.writer = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_write_message() {
        let tmp_dir = TempDir::new().unwrap();
        let mut writer = FileWriter::new(tmp_dir.path(), "test-feed");

        let msg = Message::new("test-feed", br#"{"price": 100}"#.to_vec());

        writer.write(&msg).await.unwrap();
        writer.close().await.unwrap();

        // Find today's date directory
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let expected_path = tmp_dir.path().join(&today).join("test-feed.jsonl");
        assert!(expected_path.exists());

        let content = fs::read_to_string(expected_path).unwrap();
        assert!(content.contains("\"price\": 100"));
    }

    #[tokio::test]
    async fn test_write_preserves_raw_bytes() {
        let tmp_dir = TempDir::new().unwrap();
        let mut writer = FileWriter::new(tmp_dir.path(), "test-feed");

        // Raw bytes - no JSON parsing
        let raw_data = br#"{"ticker":"BTCUSD","price":42000.50}"#;
        let msg = Message::new("test-feed", raw_data.to_vec());

        writer.write(&msg).await.unwrap();
        writer.close().await.unwrap();

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let expected_path = tmp_dir.path().join(&today).join("test-feed.jsonl");
        let content = fs::read_to_string(expected_path).unwrap();

        // Should contain raw data unchanged
        assert!(content.contains(r#""ticker":"BTCUSD""#));
        assert!(content.contains(r#""price":42000.50"#));
    }
}
