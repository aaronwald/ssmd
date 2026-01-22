//! NATS Writer - publishes raw JSON messages to NATS
//!
//! Passes through incoming JSON messages from connectors directly to NATS.
//! No transformation - raw bytes are preserved for archiving.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use tracing::{trace, warn};

use ssmd_middleware::{SubjectBuilder, Transport};

use crate::error::WriterError;
use crate::kalshi::messages::WsMessage;
use crate::message::Message;
use crate::traits::Writer;

/// Writer that publishes raw JSON messages to NATS
pub struct NatsWriter {
    transport: Arc<dyn Transport>,
    subjects: SubjectBuilder,
    message_count: u64,
    /// Optional filter for lifecycle events by series ticker.
    /// Extracts series from market_ticker (first segment before '-') and checks HashSet.
    /// If None, all lifecycle events are published.
    series_filter: Option<HashSet<String>>,
    /// Counters for filtered messages
    lifecycle_filtered_count: u64,
}

impl NatsWriter {
    /// Create a new NatsWriter with default subject prefix: {env_name}.{feed_name}
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<String>,
        feed_name: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::new(env_name, feed_name),
            message_count: 0,
            series_filter: None,
            lifecycle_filtered_count: 0,
        }
    }

    /// Create a new NatsWriter with a custom subject prefix and stream name.
    /// Use this for sharding connectors to different NATS streams.
    ///
    /// Example:
    /// ```ignore
    /// let writer = NatsWriter::with_prefix(transport, "prod.kalshi.main", "PROD_KALSHI");
    /// // Publishes to: prod.kalshi.main.json.trade.<ticker>
    /// ```
    pub fn with_prefix(
        transport: Arc<dyn Transport>,
        subject_prefix: impl Into<String>,
        stream_name: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::with_prefix(subject_prefix, stream_name),
            message_count: 0,
            series_filter: None,
            lifecycle_filtered_count: 0,
        }
    }

    /// Set a series filter for lifecycle events.
    /// Extracts series from market_ticker (e.g., "KXBTCD" from "KXBTCD-26JAN25-T95000")
    /// and only publishes if the series is in this set.
    /// If not set, all lifecycle events are published.
    pub fn with_series_filter(mut self, series: HashSet<String>) -> Self {
        self.series_filter = Some(series);
        self
    }

    /// Extract series ticker from market ticker (first segment before '-')
    fn extract_series(market_ticker: &str) -> &str {
        market_ticker.split('-').next().unwrap_or(market_ticker)
    }

    /// Get count of published messages
    pub fn message_count(&self) -> u64 {
        self.message_count
    }

    /// Get count of lifecycle messages that were filtered out
    pub fn lifecycle_filtered_count(&self) -> u64 {
        self.lifecycle_filtered_count
    }
}

#[async_trait]
impl Writer for NatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        // Parse just enough to extract message type and ticker
        let ws_msg: WsMessage = match serde_json::from_slice(&msg.data) {
            Ok(m) => m,
            Err(e) => {
                // Fail loudly on parse errors - indicates a bug in WsMessage enum
                let preview: String = String::from_utf8_lossy(&msg.data)
                    .chars()
                    .take(500)
                    .collect();
                return Err(WriterError::WriteFailed(format!(
                    "Failed to parse message: {}. Preview: {}",
                    e, preview
                )));
            }
        };

        let subject = match &ws_msg {
            WsMessage::Trade { msg: trade_data } => {
                self.subjects.json_trade(&trade_data.market_ticker)
            }
            WsMessage::Ticker { msg: ticker_data } => {
                self.subjects.json_ticker(&ticker_data.market_ticker)
            }
            WsMessage::OrderbookSnapshot { msg: ob_data } => {
                self.subjects.json_orderbook(&ob_data.market_ticker)
            }
            WsMessage::OrderbookDelta { msg: ob_data } => {
                self.subjects.json_orderbook(&ob_data.market_ticker)
            }
            WsMessage::MarketLifecycleV2 { msg: lifecycle_data, .. } => {
                // Apply series filter if configured
                if let Some(ref filter) = self.series_filter {
                    let series = Self::extract_series(&lifecycle_data.market_ticker);
                    if !filter.contains(series) {
                        trace!(
                            market_ticker = %lifecycle_data.market_ticker,
                            series = %series,
                            "Lifecycle event filtered out (series not in filter)"
                        );
                        self.lifecycle_filtered_count += 1;
                        return Ok(());
                    }
                }
                self.subjects.json_lifecycle(&lifecycle_data.market_ticker)
            }
            WsMessage::EventLifecycle { msg: event_data, .. } => {
                // Apply series filter to event lifecycle if configured
                if let Some(ref filter) = self.series_filter {
                    if let Some(ref series_ticker) = event_data.series_ticker {
                        if !filter.contains(series_ticker.as_str()) {
                            trace!(
                                event_ticker = %event_data.event_ticker,
                                series_ticker = %series_ticker,
                                "Event lifecycle filtered out (series not in filter)"
                            );
                            self.lifecycle_filtered_count += 1;
                            return Ok(());
                        }
                    }
                }
                self.subjects.json_event_lifecycle(&event_data.event_ticker)
            }
            WsMessage::Subscribed { .. } | WsMessage::Unsubscribed { .. } => {
                // Control messages, don't publish
                return Ok(());
            }
            WsMessage::Ok { .. } => {
                // Control message, don't publish
                return Ok(());
            }
            WsMessage::Error { id, msg } => {
                let code = msg.as_ref().map(|m| m.code);
                let error_msg = msg.as_ref().map(|m| m.msg.as_str());
                warn!(?id, ?code, ?error_msg, "Error message received from Kalshi");
                return Ok(());
            }
            WsMessage::Unknown => {
                warn!("Unknown message type received");
                return Ok(());
            }
        };

        // Publish raw bytes - no transformation
        self.transport
            .publish(&subject, Bytes::from(msg.data.clone()))
            .await
            .map_err(|e| WriterError::WriteFailed(format!("NATS publish failed: {}", e)))?;

        self.message_count += 1;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), WriterError> {
        trace!(messages = self.message_count, "NatsWriter closing");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_middleware::InMemoryTransport;

    #[tokio::test]
    async fn test_publish_trade_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        // Subscribe to exact subject
        let mut sub = transport
            .subscribe("dev.kalshi.json.trade.KXTEST-123")
            .await
            .unwrap();

        let trade_json = br#"{"type":"trade","sid":2,"seq":1,"msg":{"market_ticker":"KXTEST-123","price":50,"count":10,"side":"yes","ts":1732579880}}"#;
        let msg = Message::new("kalshi", trade_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.kalshi.json.trade.KXTEST-123");
        // Raw JSON preserved
        assert_eq!(received.payload.as_ref(), trade_json);
    }

    #[tokio::test]
    async fn test_publish_ticker_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        let mut sub = transport
            .subscribe("dev.kalshi.json.ticker.KXTEST-456")
            .await
            .unwrap();

        let ticker_json = br#"{"type":"ticker","sid":1,"msg":{"market_ticker":"KXTEST-456","yes_bid":45,"yes_ask":46,"price":45,"volume":1000,"open_interest":500,"ts":1732579880}}"#;
        let msg = Message::new("kalshi", ticker_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.kalshi.json.ticker.KXTEST-456");
        assert_eq!(received.payload.as_ref(), ticker_json);
    }

    #[tokio::test]
    async fn test_skip_control_messages() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        let subscribed_json = br#"{"type":"subscribed","id":1}"#;
        let msg = Message::new("kalshi", subscribed_json.to_vec());

        // Should not error
        writer.write(&msg).await.unwrap();

        // No messages published
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_message_count() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        // Need to subscribe to receive
        let _sub = transport
            .subscribe("dev.kalshi.json.trade.KXTEST-123")
            .await
            .unwrap();

        let trade_json = br#"{"type":"trade","sid":2,"seq":1,"msg":{"market_ticker":"KXTEST-123","price":50,"count":10,"side":"yes","ts":1732579880}}"#;
        let msg = Message::new("kalshi", trade_json.to_vec());

        writer.write(&msg).await.unwrap();
        writer.write(&msg).await.unwrap();

        assert_eq!(writer.message_count(), 2);
    }
}
