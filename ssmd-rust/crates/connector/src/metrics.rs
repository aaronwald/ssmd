//! Prometheus metrics for the connector
//!
//! Provides per-shard metrics for monitoring WebSocket connections and message flow.

use once_cell::sync::Lazy;
use prometheus::{
    register_gauge_vec, register_int_counter_vec, register_int_gauge_vec, Encoder, GaugeVec,
    IntCounterVec, IntGaugeVec, TextEncoder,
};

/// Labels used for metrics
const LABEL_FEED: &str = "feed";
const LABEL_CATEGORY: &str = "category";
const LABEL_SHARD: &str = "shard";
const LABEL_MESSAGE_TYPE: &str = "message_type";

/// Total messages received per shard and message type
static MESSAGES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_connector_messages_total",
        "Total messages received by the connector",
        &[LABEL_FEED, LABEL_CATEGORY, LABEL_SHARD, LABEL_MESSAGE_TYPE]
    )
    .expect("Failed to register messages_total metric")
});

/// Last activity timestamp (epoch seconds) per shard
static LAST_ACTIVITY_TIMESTAMP: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        "ssmd_connector_last_activity_timestamp",
        "Unix timestamp of last WebSocket activity per shard",
        &[LABEL_FEED, LABEL_CATEGORY, LABEL_SHARD]
    )
    .expect("Failed to register last_activity_timestamp metric")
});

/// WebSocket connection status per shard (1 = connected, 0 = disconnected)
static WEBSOCKET_CONNECTED: Lazy<IntGaugeVec> = Lazy::new(|| {
    register_int_gauge_vec!(
        "ssmd_connector_websocket_connected",
        "WebSocket connection status per shard (1=connected, 0=disconnected)",
        &[LABEL_FEED, LABEL_CATEGORY, LABEL_SHARD]
    )
    .expect("Failed to register websocket_connected metric")
});

/// Total number of shards for this connector
static SHARDS_TOTAL: Lazy<IntGaugeVec> = Lazy::new(|| {
    register_int_gauge_vec!(
        "ssmd_connector_shards_total",
        "Total number of WebSocket shards",
        &[LABEL_FEED, LABEL_CATEGORY]
    )
    .expect("Failed to register shards_total metric")
});

/// Markets subscribed per shard
static MARKETS_SUBSCRIBED: Lazy<IntGaugeVec> = Lazy::new(|| {
    register_int_gauge_vec!(
        "ssmd_connector_markets_subscribed",
        "Number of markets subscribed per shard",
        &[LABEL_FEED, LABEL_CATEGORY, LABEL_SHARD]
    )
    .expect("Failed to register markets_subscribed metric")
});

/// Idle seconds since last message per shard
static IDLE_SECONDS: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        "ssmd_connector_idle_seconds",
        "Seconds since last message received per shard",
        &[LABEL_FEED, LABEL_CATEGORY, LABEL_SHARD]
    )
    .expect("Failed to register idle_seconds metric")
});

/// Handle for recording metrics for a specific connector instance
#[derive(Clone)]
pub struct ConnectorMetrics {
    feed: String,
    category: String,
}

impl ConnectorMetrics {
    /// Create a new metrics handle for a connector
    pub fn new(feed: impl Into<String>, category: impl Into<String>) -> Self {
        Self {
            feed: feed.into(),
            category: category.into(),
        }
    }

    /// Record total number of shards
    pub fn set_shards_total(&self, count: usize) {
        SHARDS_TOTAL
            .with_label_values(&[&self.feed, &self.category])
            .set(count as i64);
    }

    /// Record number of markets subscribed for a shard
    pub fn set_markets_subscribed(&self, shard_id: usize, count: usize) {
        MARKETS_SUBSCRIBED
            .with_label_values(&[&self.feed, &self.category, &shard_id.to_string()])
            .set(count as i64);
    }

    /// Mark a shard as connected
    pub fn set_shard_connected(&self, shard_id: usize) {
        WEBSOCKET_CONNECTED
            .with_label_values(&[&self.feed, &self.category, &shard_id.to_string()])
            .set(1);
    }

    /// Mark a shard as disconnected
    pub fn set_shard_disconnected(&self, shard_id: usize) {
        WEBSOCKET_CONNECTED
            .with_label_values(&[&self.feed, &self.category, &shard_id.to_string()])
            .set(0);
    }

    /// Create a shard-specific metrics handle
    pub fn for_shard(&self, shard_id: usize) -> ShardMetrics {
        ShardMetrics {
            feed: self.feed.clone(),
            category: self.category.clone(),
            shard_id,
        }
    }
}

/// Handle for recording metrics for a specific shard
#[derive(Clone)]
pub struct ShardMetrics {
    feed: String,
    category: String,
    shard_id: usize,
}

impl ShardMetrics {
    /// Increment message counter for a specific message type
    pub fn inc_message(&self, message_type: &str) {
        MESSAGES_TOTAL
            .with_label_values(&[
                &self.feed,
                &self.category,
                &self.shard_id.to_string(),
                message_type,
            ])
            .inc();
    }

    /// Record ticker message received
    pub fn inc_ticker(&self) {
        self.inc_message("ticker");
    }

    /// Record trade message received
    pub fn inc_trade(&self) {
        self.inc_message("trade");
    }

    /// Record orderbook message received
    pub fn inc_orderbook(&self) {
        self.inc_message("orderbook");
    }

    /// Record lifecycle message received
    pub fn inc_lifecycle(&self) {
        self.inc_message("lifecycle");
    }

    /// Record event lifecycle message received
    pub fn inc_event_lifecycle(&self) {
        self.inc_message("event_lifecycle");
    }

    /// Update last activity timestamp
    pub fn set_last_activity(&self, epoch_secs: f64) {
        LAST_ACTIVITY_TIMESTAMP
            .with_label_values(&[&self.feed, &self.category, &self.shard_id.to_string()])
            .set(epoch_secs);
    }

    /// Update idle seconds
    pub fn set_idle_seconds(&self, seconds: f64) {
        IDLE_SECONDS
            .with_label_values(&[&self.feed, &self.category, &self.shard_id.to_string()])
            .set(seconds);
    }

    /// Mark shard as connected
    pub fn set_connected(&self) {
        WEBSOCKET_CONNECTED
            .with_label_values(&[&self.feed, &self.category, &self.shard_id.to_string()])
            .set(1);
    }

    /// Mark shard as disconnected
    pub fn set_disconnected(&self) {
        WEBSOCKET_CONNECTED
            .with_label_values(&[&self.feed, &self.category, &self.shard_id.to_string()])
            .set(0);
    }

    /// Get the current number of markets subscribed for this shard
    pub fn get_markets_subscribed(&self) -> usize {
        MARKETS_SUBSCRIBED
            .with_label_values(&[&self.feed, &self.category, &self.shard_id.to_string()])
            .get() as usize
    }

    /// Set the number of markets subscribed for this shard
    pub fn set_markets_subscribed(&self, count: usize) {
        MARKETS_SUBSCRIBED
            .with_label_values(&[&self.feed, &self.category, &self.shard_id.to_string()])
            .set(count as i64);
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
    fn test_connector_metrics_creation() {
        let metrics = ConnectorMetrics::new("kalshi", "economics");
        metrics.set_shards_total(2);
        metrics.set_markets_subscribed(0, 256);
        metrics.set_shard_connected(0);
    }

    #[test]
    fn test_shard_metrics() {
        let connector_metrics = ConnectorMetrics::new("kalshi", "test");
        let shard_metrics = connector_metrics.for_shard(0);

        shard_metrics.inc_ticker();
        shard_metrics.inc_trade();
        shard_metrics.set_last_activity(1234567890.0);
        shard_metrics.set_idle_seconds(5.0);
        shard_metrics.set_connected();
    }

    #[test]
    fn test_encode_metrics() {
        let result = encode_metrics();
        assert!(result.is_ok());
        let output = result.unwrap();
        // Should contain some metric output (may be empty if no metrics recorded yet in this test)
        assert!(output.is_empty() || output.contains("ssmd_connector"));
    }
}
