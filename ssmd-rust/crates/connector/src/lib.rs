//! ssmd-connector: Market data collection runtime components
//!
//! This crate provides the core components for connecting to market data sources,
//! processing messages, and writing to various destinations.

pub mod error;
pub mod flusher;
pub mod kalshi;
pub mod kraken;
pub mod message;
pub mod metrics;
pub mod nats_writer;
pub mod publisher;
pub mod resolver;
pub mod ring_buffer;
pub mod runner;
pub mod secmaster;
pub mod server;
pub mod traits;
pub mod websocket;
// writer.rs kept for ring buffer integration tests but not exported
// TODO: Delete in next major version when archiver replaces file writer
#[allow(dead_code)]
mod writer;

pub use error::{ConnectorError, ResolverError, WriterError};
pub use flusher::DiskFlusher;
pub use message::Message;
pub use metrics::{encode_metrics, ConnectorMetrics, ShardMetrics};
pub use nats_writer::NatsWriter;
pub use publisher::{Publisher, TradeData, TradeSide};
pub use resolver::EnvResolver;
pub use ring_buffer::{RingBuffer, RING_SIZE, RING_SLOTS, SLOT_SIZE};
pub use runner::Runner;
pub use secmaster::{SecmasterClient, SecmasterError};
pub use server::{create_router, run_server, ServerState};
pub use traits::{Connector, KeyResolver, Writer};
pub use websocket::WebSocketConnector;

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_pipeline() -> (Arc<RingBuffer>, TempDir) {
        let tmp = TempDir::new().unwrap();
        let ring_path = tmp.path().join("ring.buf");
        let ring = Arc::new(RingBuffer::new(&ring_path).unwrap());
        (ring, tmp)
    }

    #[test]
    fn test_full_pipeline_integration() {
        let (ring, tmp) = create_test_pipeline();
        let shutdown = Arc::new(AtomicBool::new(false));

        let ring_producer = Arc::clone(&ring);
        let ring_flusher = Arc::clone(&ring);
        let shutdown_flusher = Arc::clone(&shutdown);

        // Spawn flusher thread
        let flusher_handle = thread::spawn(move || {
            let mut flusher = flusher::DiskFlusher::new(
                ring_flusher,
                tmp.path().to_path_buf(),
                "pipeline-test".to_string(),
            );
            flusher.run(shutdown_flusher);
            tmp // Return tmp to keep it alive
        });

        // Producer writes 1000 messages
        const NUM_MESSAGES: usize = 1000;
        for i in 0..NUM_MESSAGES {
            let msg = format!("{{\"seq\":{}}}", i);
            while !ring_producer.try_write(msg.as_bytes()) {
                thread::yield_now();
            }
        }

        // Small delay to let flusher catch up, then shutdown
        thread::sleep(std::time::Duration::from_millis(50));
        shutdown.store(true, Ordering::Relaxed);

        let tmp = flusher_handle.join().unwrap();

        // Verify all messages in output file
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let output_path = tmp.path().join(&today).join("pipeline-test.jsonl");
        let content = fs::read_to_string(&output_path).unwrap();
        let lines: Vec<_> = content.lines().collect();

        assert_eq!(lines.len(), NUM_MESSAGES, "All {} messages should be written", NUM_MESSAGES);

        // Verify ordering
        for (i, line) in lines.iter().enumerate() {
            assert!(line.contains(&format!("\"seq\":{}", i)), "Message {} should be in order", i);
        }
    }

    #[test]
    fn test_concurrent_producer_flusher() {
        let (ring, tmp) = create_test_pipeline();
        let shutdown = Arc::new(AtomicBool::new(false));

        let ring_producer = Arc::clone(&ring);
        let ring_flusher = Arc::clone(&ring);
        let shutdown_flusher = Arc::clone(&shutdown);

        // Spawn flusher thread
        let flusher_handle = thread::spawn(move || {
            let mut flusher = flusher::DiskFlusher::new(
                ring_flusher,
                tmp.path().to_path_buf(),
                "concurrent-test".to_string(),
            );
            flusher.run(shutdown_flusher);
            tmp
        });

        // Producer writes in bursts with small delays
        let producer_handle = thread::spawn(move || {
            let mut total = 0;
            for burst in 0..10 {
                for i in 0..100 {
                    let msg = format!("{{\"burst\":{},\"msg\":{}}}", burst, i);
                    while !ring_producer.try_write(msg.as_bytes()) {
                        thread::yield_now();
                    }
                    total += 1;
                }
                thread::sleep(std::time::Duration::from_millis(5));
            }
            total
        });

        let total_sent = producer_handle.join().unwrap();
        thread::sleep(std::time::Duration::from_millis(50));
        shutdown.store(true, Ordering::Relaxed);

        let tmp = flusher_handle.join().unwrap();

        // Verify all messages received
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let output_path = tmp.path().join(&today).join("concurrent-test.jsonl");
        let content = fs::read_to_string(&output_path).unwrap();
        let lines: Vec<_> = content.lines().collect();

        assert_eq!(lines.len(), total_sent, "All {} messages should be written", total_sent);
    }

    #[test]
    fn test_shutdown_while_writing() {
        let (ring, tmp) = create_test_pipeline();
        let shutdown = Arc::new(AtomicBool::new(false));

        let ring_producer = Arc::clone(&ring);
        let ring_flusher = Arc::clone(&ring);
        let shutdown_flusher = Arc::clone(&shutdown);
        let shutdown_producer = Arc::clone(&shutdown);

        // Spawn flusher
        let flusher_handle = thread::spawn(move || {
            let mut flusher = flusher::DiskFlusher::new(
                ring_flusher,
                tmp.path().to_path_buf(),
                "shutdown-test".to_string(),
            );
            flusher.run(shutdown_flusher);
            tmp
        });

        // Producer writes until shutdown
        let producer_handle = thread::spawn(move || {
            let mut count = 0;
            while !shutdown_producer.load(Ordering::Relaxed) {
                let msg = format!("{{\"n\":{}}}", count);
                if ring_producer.try_write(msg.as_bytes()) {
                    count += 1;
                }
                if count >= 500 {
                    break; // Write at least 500 messages
                }
            }
            count
        });

        // Let producer run for a bit, then shutdown
        thread::sleep(std::time::Duration::from_millis(20));
        shutdown.store(true, Ordering::Relaxed);

        let messages_sent = producer_handle.join().unwrap();
        let tmp = flusher_handle.join().unwrap();

        // Verify all queued messages were flushed
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let output_path = tmp.path().join(&today).join("shutdown-test.jsonl");
        let content = fs::read_to_string(&output_path).unwrap();
        let lines: Vec<_> = content.lines().collect();

        assert_eq!(lines.len(), messages_sent, "All {} sent messages should be flushed on shutdown", messages_sent);
    }

    #[test]
    #[ignore] // Flaky due to race condition - flusher may drain before assertion
    fn test_producer_resumes_after_drain() {
        let (ring, tmp) = create_test_pipeline();
        let shutdown = Arc::new(AtomicBool::new(false));

        let ring_producer = Arc::clone(&ring);
        let ring_flusher = Arc::clone(&ring);
        let shutdown_flusher = Arc::clone(&shutdown);

        // Spawn flusher
        let flusher_handle = thread::spawn(move || {
            let mut flusher = flusher::DiskFlusher::new(
                ring_flusher,
                tmp.path().to_path_buf(),
                "resume-test".to_string(),
            );
            flusher.run(shutdown_flusher);
            tmp
        });

        // Fill ring buffer until full
        let mut phase1_count = 0;
        while ring_producer.try_write(format!("{{\"phase\":1,\"n\":{}}}", phase1_count).as_bytes()) {
            phase1_count += 1;
        }
        assert!(ring.is_full(), "Ring should be full after phase 1");

        // Wait for flusher to drain some
        thread::sleep(std::time::Duration::from_millis(20));

        // Should be able to write again
        let mut phase2_count = 0;
        for _ in 0..100 {
            if ring_producer.try_write(format!("{{\"phase\":2,\"n\":{}}}", phase2_count).as_bytes()) {
                phase2_count += 1;
            }
        }
        assert!(phase2_count > 0, "Should have written some phase 2 messages");

        shutdown.store(true, Ordering::Relaxed);
        let tmp = flusher_handle.join().unwrap();

        // Verify all messages
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let output_path = tmp.path().join(&today).join("resume-test.jsonl");
        let content = fs::read_to_string(&output_path).unwrap();

        assert!(content.contains("\"phase\":1"), "Should have phase 1 messages");
        assert!(content.contains("\"phase\":2"), "Should have phase 2 messages");
    }
}
