//! NATS Writer - publishes raw JSON messages to NATS
//!
//! Passes through incoming JSON messages from connectors directly to NATS.
//! No transformation - raw bytes are preserved for archiving.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{trace, warn};

use ssmd_middleware::{SubjectBuilder, Transport};

use crate::error::WriterError;
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
        env_name: impl Into<Arc<str>>,
        feed_name: impl Into<Arc<str>>,
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
        subject_prefix: impl Into<Arc<str>>,
        stream_name: impl Into<Arc<str>>,
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

#[derive(Deserialize)]
struct PartialWsMessage<'a> {
    #[serde(rename = "type")]
    msg_type: &'a str,
    id: Option<u64>,
    #[serde(borrow)]
    msg: Option<PartialMsgData<'a>>,
}

#[derive(Deserialize)]
struct PartialMsgData<'a> {
    market_ticker: Option<&'a str>,
    event_ticker: Option<&'a str>,
    series_ticker: Option<&'a str>,
    code: Option<i64>,
    #[serde(rename = "msg")]
    error_msg: Option<&'a str>,
}

#[async_trait]
impl Writer for NatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        // FAST PATH: Parse just enough to extract message type and ticker using borrowed strings.
        // Bypasses full WsMessage enum and serde_json::Value overhead.
        let partial: PartialWsMessage = match serde_json::from_slice(&msg.data) {
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

        let subject = match partial.msg_type {
            "trade" => {
                let ticker = partial.msg.as_ref().and_then(|m| m.market_ticker).unwrap_or("");
                if ticker.is_empty() {
                    warn!(msg_type = %partial.msg_type, "Missing market_ticker, skipping");
                    return Ok(());
                }
                self.subjects.json_trade(ticker)
            }
            "ticker" => {
                let ticker = partial.msg.as_ref().and_then(|m| m.market_ticker).unwrap_or("");
                if ticker.is_empty() {
                    warn!(msg_type = %partial.msg_type, "Missing market_ticker, skipping");
                    return Ok(());
                }
                self.subjects.json_ticker(ticker)
            }
            "orderbook_snapshot" => {
                let ticker = partial.msg.as_ref().and_then(|m| m.market_ticker).unwrap_or("");
                if ticker.is_empty() {
                    warn!(msg_type = %partial.msg_type, "Missing market_ticker, skipping");
                    return Ok(());
                }
                self.subjects.json_orderbook(ticker)
            }
            "orderbook_delta" => {
                let ticker = partial.msg.as_ref().and_then(|m| m.market_ticker).unwrap_or("");
                if ticker.is_empty() {
                    warn!(msg_type = %partial.msg_type, "Missing market_ticker, skipping");
                    return Ok(());
                }
                self.subjects.json_orderbook(ticker)
            }
            "market_lifecycle_v2" => {
                let ticker = partial.msg.as_ref().and_then(|m| m.market_ticker).unwrap_or("");
                if ticker.is_empty() {
                    warn!(msg_type = %partial.msg_type, "Missing market_ticker, skipping");
                    return Ok(());
                }
                // Apply series filter if configured
                if let Some(ref filter) = self.series_filter {
                    let series = Self::extract_series(ticker);
                    if !filter.contains(series) {
                        trace!(
                            market_ticker = %ticker,
                            series = %series,
                            "Lifecycle event filtered out (series not in filter)"
                        );
                        self.lifecycle_filtered_count += 1;
                        return Ok(());
                    }
                }
                self.subjects.json_lifecycle(ticker)
            }
            "event_lifecycle" => {
                let ticker = partial.msg.as_ref().and_then(|m| m.event_ticker).unwrap_or("");
                if ticker.is_empty() {
                    warn!(msg_type = %partial.msg_type, "Missing event_ticker, skipping");
                    return Ok(());
                }
                // Apply series filter to event lifecycle if configured
                if let Some(ref filter) = self.series_filter {
                    if let Some(series_ticker) = partial.msg.as_ref().and_then(|m| m.series_ticker) {
                        if !filter.contains(series_ticker) {
                            trace!(
                                event_ticker = %ticker,
                                series_ticker = %series_ticker,
                                "Event lifecycle filtered out (series not in filter)"
                            );
                            self.lifecycle_filtered_count += 1;
                            return Ok(());
                        }
                    }
                }
                self.subjects.json_event_lifecycle(ticker)
            }
            "subscribed" | "unsubscribed" | "ok" => {
                // Control messages, don't publish
                return Ok(());
            }
            "error" => {
                let code = partial.msg.as_ref().and_then(|m| m.code);
                let error_msg = partial.msg.as_ref().and_then(|m| m.error_msg);
                warn!(id = ?partial.id, ?code, ?error_msg, "Error message received from Kalshi");
                return Ok(());
            }
            _ => {
                warn!(msg_type = %partial.msg_type, "Unknown message type received");
                return Ok(());
            }
        };

        // Publish raw bytes - no transformation
        self.transport
            .publish(&subject, msg.data.clone())
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
