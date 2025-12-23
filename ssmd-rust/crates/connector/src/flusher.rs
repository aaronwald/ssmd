//! Disk flusher that drains ring buffer and writes to date-partitioned files
//!
//! Runs on dedicated std::thread to avoid tokio runtime overhead.
//! Wall-clock timestamps applied here (syscall OK - we're doing disk I/O anyway).

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::ring_buffer::RingBuffer;

/// Batch size: drain up to this many messages before yielding
const BATCH_SIZE: usize = 64;

/// Sleep duration when ring is empty (100μs)
const EMPTY_SLEEP_MICROS: u64 = 100;

/// BufWriter capacity (64KB)
const WRITE_BUFFER_SIZE: usize = 65536;

/// Disk flusher that consumes from ring buffer and writes to JSONL files
pub struct DiskFlusher {
    ring: Arc<RingBuffer>,
    base_dir: PathBuf,
    feed_name: String,
    current_writer: Option<BufWriter<File>>,
    current_date: String,
}

impl DiskFlusher {
    /// Create a new disk flusher
    pub fn new(ring: Arc<RingBuffer>, base_dir: PathBuf, feed_name: String) -> Self {
        Self {
            ring,
            base_dir,
            feed_name,
            current_writer: None,
            current_date: String::new(),
        }
    }

    /// Run the flusher loop until shutdown signal
    /// Call this from a dedicated std::thread
    pub fn run(&mut self, shutdown: Arc<AtomicBool>) {
        while !shutdown.load(Ordering::Relaxed) {
            let count = self.drain_batch();

            if count > 0 {
                self.flush();
            } else {
                // Ring empty, sleep briefly to avoid busy-spin
                std::thread::sleep(std::time::Duration::from_micros(EMPTY_SLEEP_MICROS));
            }
        }

        // Shutdown: drain all remaining messages
        self.drain_all();
        self.flush();
    }

    /// Drain up to BATCH_SIZE messages from ring
    fn drain_batch(&mut self) -> usize {
        let mut count = 0;
        while count < BATCH_SIZE {
            if let Some(payload) = self.ring.try_read() {
                self.write_message(&payload);
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Drain all remaining messages from ring
    fn drain_all(&mut self) {
        while let Some(payload) = self.ring.try_read() {
            self.write_message(&payload);
        }
    }

    /// Write a single message to the current file
    fn write_message(&mut self, payload: &[u8]) {
        // Wall-clock timestamp - syscall OK here, we're about to do disk I/O
        let now = chrono::Utc::now();
        let date = now.format("%Y-%m-%d").to_string();

        // Rotate file if date changed
        if date != self.current_date {
            self.rotate_file(&date);
        }

        if let Some(ref mut writer) = self.current_writer {
            // Write timestamp and payload as JSONL
            let ts = now.to_rfc3339();
            let _ = write!(writer, "{{\"ts\":\"{}\",\"data\":", ts);
            let _ = writer.write_all(payload);
            let _ = writeln!(writer, "}}");
        }
    }

    /// Flush current writer
    fn flush(&mut self) {
        if let Some(ref mut writer) = self.current_writer {
            let _ = writer.flush();
        }
    }

    /// Rotate to a new file for the given date
    fn rotate_file(&mut self, date: &str) {
        // Flush and close current writer
        self.flush();

        // Create date directory
        let dir = self.base_dir.join(date);
        if let Err(e) = fs::create_dir_all(&dir) {
            tracing::error!(error = %e, "Failed to create directory");
            return;
        }

        // Open new file
        let path = dir.join(format!("{}.jsonl", self.feed_name));
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                self.current_writer = Some(BufWriter::with_capacity(WRITE_BUFFER_SIZE, file));
                self.current_date = date.to_string();
            }
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "Failed to open file");
            }
        }
    }
}

impl Drop for DiskFlusher {
    fn drop(&mut self) {
        // Safety net: flush on drop
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_setup() -> (Arc<RingBuffer>, DiskFlusher, TempDir) {
        let tmp = TempDir::new().unwrap();
        let ring_path = tmp.path().join("ring.buf");
        let ring = Arc::new(RingBuffer::new(&ring_path).unwrap());
        let flusher = DiskFlusher::new(ring.clone(), tmp.path().to_path_buf(), "test-feed".to_string());
        (ring, flusher, tmp)
    }

    #[test]
    fn test_flusher_writes_to_file() {
        let (ring, mut flusher, tmp) = create_test_setup();

        // Write some messages
        ring.try_write(b"{\"price\":100}");
        ring.try_write(b"{\"price\":101}");

        // Drain and flush
        flusher.drain_batch();
        flusher.flush();

        // Find the output file
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let output_path = tmp.path().join(&today).join("test-feed.jsonl");

        assert!(output_path.exists(), "Output file should exist");

        let content = fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("\"price\":100"), "Should contain first message");
        assert!(content.contains("\"price\":101"), "Should contain second message");
        assert!(content.contains("\"ts\":"), "Should have timestamps");
    }

    #[test]
    fn test_flusher_drains_on_shutdown() {
        let (ring, mut flusher, tmp) = create_test_setup();
        let shutdown = Arc::new(AtomicBool::new(false));

        // Write messages
        for i in 0..10 {
            ring.try_write(format!("{{\"n\":{}}}", i).as_bytes());
        }

        // Signal shutdown immediately
        shutdown.store(true, Ordering::Relaxed);

        // Run flusher - should drain all and exit
        flusher.run(shutdown);

        // Verify all messages written
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let output_path = tmp.path().join(&today).join("test-feed.jsonl");
        let content = fs::read_to_string(&output_path).unwrap();
        let lines: Vec<_> = content.lines().collect();

        assert_eq!(lines.len(), 10, "All 10 messages should be written");
    }

    #[test]
    fn test_flusher_batches_writes() {
        let (ring, mut flusher, _tmp) = create_test_setup();

        // Write more than batch size
        for i in 0..100 {
            ring.try_write(format!("{{\"i\":{}}}", i).as_bytes());
        }

        // First batch should drain 64
        let count = flusher.drain_batch();
        assert_eq!(count, BATCH_SIZE);

        // Second batch should drain remaining 36
        let count = flusher.drain_batch();
        assert_eq!(count, 36);

        // Third batch should be empty
        let count = flusher.drain_batch();
        assert_eq!(count, 0);
    }
}
