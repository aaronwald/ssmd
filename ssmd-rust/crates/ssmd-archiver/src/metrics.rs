//! Prometheus metrics for the archiver
//!
//! Provides per-stream metrics for monitoring NATS archival.

use once_cell::sync::Lazy;
use prometheus::{
    register_gauge_vec, register_int_counter_vec, register_int_gauge, Encoder, GaugeVec,
    IntCounterVec, IntGauge, TextEncoder,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const LABEL_FEED: &str = "feed";
const LABEL_STREAM: &str = "stream";
const LABEL_MESSAGE_TYPE: &str = "message_type";

/// Total messages archived per stream and message type
static MESSAGES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_archiver_messages_total",
        "Total messages written by the archiver",
        &[LABEL_FEED, LABEL_STREAM, LABEL_MESSAGE_TYPE]
    )
    .expect("Failed to register messages_total metric")
});

/// Total bytes archived per stream
static BYTES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_archiver_bytes_total",
        "Total bytes written by the archiver",
        &[LABEL_FEED, LABEL_STREAM]
    )
    .expect("Failed to register bytes_total metric")
});

/// Files rotated per stream
static FILES_ROTATED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_archiver_files_rotated_total",
        "Total file rotations completed",
        &[LABEL_FEED, LABEL_STREAM]
    )
    .expect("Failed to register files_rotated_total metric")
});

/// Validation failures per stream
static VALIDATION_FAILURES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_archiver_validation_failures_total",
        "Total validation failures",
        &[LABEL_FEED, LABEL_STREAM]
    )
    .expect("Failed to register validation_failures_total metric")
});

/// Parse failures per stream
static PARSE_FAILURES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_archiver_parse_failures_total",
        "Total parse failures",
        &[LABEL_FEED, LABEL_STREAM]
    )
    .expect("Failed to register parse_failures_total metric")
});

/// NATS sequence gaps per stream
static GAPS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_archiver_gaps_total",
        "Total NATS sequence gaps detected",
        &[LABEL_FEED, LABEL_STREAM]
    )
    .expect("Failed to register gaps_total metric")
});

/// Number of active stream subscriptions
static ACTIVE_STREAMS: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge!(
        "ssmd_archiver_active_streams",
        "Number of active stream subscriptions"
    )
    .expect("Failed to register active_streams metric")
});

/// Last message timestamp per stream (epoch seconds)
static LAST_MESSAGE_TIMESTAMP: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        "ssmd_archiver_last_message_timestamp",
        "Unix timestamp of last message archived per stream",
        &[LABEL_FEED, LABEL_STREAM]
    )
    .expect("Failed to register last_message_timestamp metric")
});

/// Handle for recording metrics for an archiver instance
#[derive(Clone)]
pub struct ArchiverMetrics {
    feed: String,
}

impl ArchiverMetrics {
    pub fn new(feed: impl Into<String>) -> Self {
        Self { feed: feed.into() }
    }

    /// Set the number of active streams
    pub fn set_active_streams(&self, count: usize) {
        ACTIVE_STREAMS.set(count as i64);
    }

    /// Create a stream-specific metrics handle
    pub fn for_stream(&self, stream: impl Into<String>) -> StreamMetrics {
        StreamMetrics {
            feed: self.feed.clone(),
            stream: stream.into(),
            total_messages: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Handle for recording metrics for a specific stream
#[derive(Clone)]
pub struct StreamMetrics {
    feed: String,
    stream: String,
    /// Local counter for efficient total message reads (avoids summing across message_type labels)
    total_messages: Arc<AtomicU64>,
}

impl StreamMetrics {
    /// Increment message counter for a specific message type
    pub fn inc_message(&self, message_type: &str) {
        MESSAGES_TOTAL
            .with_label_values(&[&self.feed, &self.stream, message_type])
            .inc();
        self.total_messages.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment bytes counter
    pub fn inc_bytes(&self, bytes: u64) {
        BYTES_TOTAL
            .with_label_values(&[&self.feed, &self.stream])
            .inc_by(bytes);
    }

    /// Increment files rotated counter
    pub fn inc_files_rotated(&self) {
        FILES_ROTATED_TOTAL
            .with_label_values(&[&self.feed, &self.stream])
            .inc();
    }

    /// Increment validation failure counter
    pub fn inc_validation_failure(&self) {
        VALIDATION_FAILURES_TOTAL
            .with_label_values(&[&self.feed, &self.stream])
            .inc();
    }

    /// Increment parse failure counter
    pub fn inc_parse_failure(&self) {
        PARSE_FAILURES_TOTAL
            .with_label_values(&[&self.feed, &self.stream])
            .inc();
    }

    /// Increment gap counter
    pub fn inc_gap(&self) {
        GAPS_TOTAL
            .with_label_values(&[&self.feed, &self.stream])
            .inc();
    }

    /// Update last message timestamp
    pub fn set_last_message_timestamp(&self, epoch_secs: f64) {
        LAST_MESSAGE_TIMESTAMP
            .with_label_values(&[&self.feed, &self.stream])
            .set(epoch_secs);
    }

    /// Get total messages count (for stats logging)
    pub fn get_messages_total(&self) -> u64 {
        self.total_messages.load(Ordering::Relaxed)
    }

    /// Get total bytes count (for stats logging)
    pub fn get_bytes_total(&self) -> u64 {
        BYTES_TOTAL
            .with_label_values(&[&self.feed, &self.stream])
            .get()
    }

    /// Get validation failure count (for stats logging)
    pub fn get_validation_failures(&self) -> u64 {
        VALIDATION_FAILURES_TOTAL
            .with_label_values(&[&self.feed, &self.stream])
            .get()
    }

    /// Get parse failure count (for stats logging)
    pub fn get_parse_failures(&self) -> u64 {
        PARSE_FAILURES_TOTAL
            .with_label_values(&[&self.feed, &self.stream])
            .get()
    }
}

/// Encode all metrics to Prometheus text format
pub fn encode_metrics() -> Result<String, prometheus::Error> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    String::from_utf8(buffer).map_err(|e| {
        prometheus::Error::Msg(format!("Failed to encode metrics as UTF-8: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archiver_metrics_creation() {
        let metrics = ArchiverMetrics::new("test-feed");
        metrics.set_active_streams(3);
    }

    #[test]
    fn test_stream_metrics() {
        let archiver_metrics = ArchiverMetrics::new("test-feed");
        let stream_metrics = archiver_metrics.for_stream("test-stream");

        stream_metrics.inc_message("trade");
        stream_metrics.inc_message("ticker");
        stream_metrics.inc_bytes(1024);
        stream_metrics.inc_files_rotated();
        stream_metrics.inc_validation_failure();
        stream_metrics.inc_parse_failure();
        stream_metrics.inc_gap();
        stream_metrics.set_last_message_timestamp(1234567890.0);

        assert_eq!(stream_metrics.get_messages_total(), 2);
        assert!(stream_metrics.get_bytes_total() >= 1024);
    }

    #[test]
    fn test_encode_metrics() {
        let result = encode_metrics();
        assert!(result.is_ok());
    }
}
