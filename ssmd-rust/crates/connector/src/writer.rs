use async_trait::async_trait;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::WriterError;
use crate::message::Message;
use crate::traits::Writer;

/// Writes messages to date-partitioned JSONL files
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

    fn get_date_from_ts(ts: &str) -> String {
        // Extract YYYY-MM-DD from ISO 8601 timestamp
        ts.get(..10).unwrap_or("unknown").to_string()
    }
}

#[async_trait]
impl Writer for FileWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        let date = Self::get_date_from_ts(&msg.ts);

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

        // Write JSON line
        if let Some(ref mut writer) = inner.writer {
            let line = serde_json::to_string(msg)
                .map_err(|e| WriterError::WriteFailed(e.to_string()))?;
            writeln!(writer, "{}", line)?;
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

        let msg = Message {
            ts: "2025-12-22T10:30:00Z".to_string(),
            feed: "test-feed".to_string(),
            data: serde_json::json!({"price": 100}),
        };

        writer.write(&msg).await.unwrap();
        writer.close().await.unwrap();

        let expected_path = tmp_dir.path().join("2025-12-22").join("test-feed.jsonl");
        assert!(expected_path.exists());

        let content = fs::read_to_string(expected_path).unwrap();
        assert!(content.contains("\"price\":100"));
    }

    #[tokio::test]
    async fn test_date_partitioning() {
        let tmp_dir = TempDir::new().unwrap();
        let mut writer = FileWriter::new(tmp_dir.path(), "test-feed");

        let msg1 = Message {
            ts: "2025-12-22T10:30:00Z".to_string(),
            feed: "test-feed".to_string(),
            data: serde_json::json!({"day": 22}),
        };
        let msg2 = Message {
            ts: "2025-12-23T10:30:00Z".to_string(),
            feed: "test-feed".to_string(),
            data: serde_json::json!({"day": 23}),
        };

        writer.write(&msg1).await.unwrap();
        writer.write(&msg2).await.unwrap();
        writer.close().await.unwrap();

        assert!(tmp_dir.path().join("2025-12-22").join("test-feed.jsonl").exists());
        assert!(tmp_dir.path().join("2025-12-23").join("test-feed.jsonl").exists());
    }
}
