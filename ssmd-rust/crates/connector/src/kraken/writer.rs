//! Kraken NATS Writer - publishes raw JSON messages to NATS
//!
//! Routes Kraken ticker and trade messages to appropriate NATS subjects.
//! Passes through raw bytes - no transformation.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{trace, warn};

use ssmd_middleware::{sanitize_subject_token, SubjectBuilder, Transport};

use crate::error::WriterError;
use crate::message::Message;
use crate::traits::Writer;

/// Writer that publishes raw Kraken JSON messages to NATS
pub struct KrakenNatsWriter {
    transport: Arc<dyn Transport>,
    subjects: SubjectBuilder,
    message_count: u64,
}

impl KrakenNatsWriter {
    /// Create a new KrakenNatsWriter with default subject prefix: {env_name}.{feed_name}
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<Arc<str>>,
        feed_name: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::new(env_name, feed_name),
            message_count: 0,
        }
    }

    /// Create a new KrakenNatsWriter with a custom subject prefix and stream name.
    pub fn with_prefix(
        transport: Arc<dyn Transport>,
        subject_prefix: impl Into<Arc<str>>,
        stream_name: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::with_prefix(subject_prefix, stream_name),
            message_count: 0,
        }
    }

    /// Get count of published messages
    pub fn message_count(&self) -> u64 {
        self.message_count
    }
}

#[derive(Deserialize)]
struct PartialKrakenMsg<'a> {
    channel: Option<&'a str>,
    #[serde(borrow)]
    data: Option<Vec<PartialKrakenData<'a>>>,
    method: Option<&'a str>,
}

#[derive(Deserialize)]
struct PartialKrakenData<'a> {
    symbol: Option<&'a str>,
}

#[async_trait]
impl Writer for KrakenNatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        // FAST PATH: Parse just enough to extract channel and symbol using borrowed strings.
        // Bypasses untagged enum and serde_json::Value overhead.
        let partial: PartialKrakenMsg = match serde_json::from_slice(&msg.data) {
            Ok(m) => m,
            Err(e) => {
                let preview: String = String::from_utf8_lossy(&msg.data)
                    .chars()
                    .take(500)
                    .collect();
                return Err(WriterError::WriteFailed(format!(
                    "Failed to parse Kraken message: {}. Preview: {}",
                    e, preview
                )));
            }
        };

        let channel = partial.channel.unwrap_or("");

        let subject = match channel {
            "trade" => {
                let symbol = partial.data.as_ref()
                    .and_then(|d| d.first())
                    .and_then(|item| item.symbol)
                    .unwrap_or("unknown");
                let sanitized = sanitize_subject_token(symbol);
                if sanitized.is_empty() {
                    warn!(channel = %channel, "Empty sanitized symbol, skipping");
                    return Ok(());
                }
                self.subjects.json_trade(&sanitized)
            }
            "ticker" => {
                let symbol = partial.data.as_ref()
                    .and_then(|d| d.first())
                    .and_then(|item| item.symbol)
                    .unwrap_or("unknown");
                let sanitized = sanitize_subject_token(symbol);
                if sanitized.is_empty() {
                    warn!(channel = %channel, "Empty sanitized symbol, skipping");
                    return Ok(());
                }
                self.subjects.json_ticker(&sanitized)
            }
            "heartbeat" => return Ok(()),
            _ => {
                // Check for non-channel messages like "pong" or "subscribe" via "method" field
                if partial.method.is_some() {
                    return Ok(());
                }
                trace!(channel = %channel, "Skipping unknown Kraken channel");
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
        trace!(messages = self.message_count, "KrakenNatsWriter closing");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_middleware::InMemoryTransport;

    #[test]
    fn test_sanitize_symbol() {
        // Now using shared sanitize_subject_token which also allows '_'
        assert_eq!(sanitize_subject_token("BTC/USD"), "BTC-USD");
        assert_eq!(sanitize_subject_token("ETH/USD"), "ETH-USD");
        assert_eq!(sanitize_subject_token("XRP/EUR"), "XRP-EUR");
        assert_eq!(sanitize_subject_token("NODASH"), "NODASH");
    }

    #[test]
    fn test_sanitize_symbol_strips_nats_wildcards() {
        // NATS wildcards and delimiters must be stripped
        assert_eq!(sanitize_subject_token("BTC.USD"), "BTCUSD");
        assert_eq!(sanitize_subject_token("BTC>USD"), "BTCUSD");
        assert_eq!(sanitize_subject_token("BTC*USD"), "BTCUSD");
        assert_eq!(sanitize_subject_token("BTC/USD.>"), "BTC-USD");
        assert_eq!(sanitize_subject_token("BTC/USD *"), "BTC-USD");
    }

    #[tokio::test]
    async fn test_publish_trade_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenNatsWriter::new(transport.clone(), "dev", "kraken");

        let mut sub = transport
            .subscribe("dev.kraken.json.trade.BTC-USD")
            .await
            .unwrap();

        let trade_json = br#"{"channel":"trade","type":"update","data":[{"symbol":"BTC/USD","side":"buy","price":97000.0,"qty":0.001,"ord_type":"market","trade_id":"12345","timestamp":"2026-02-06T12:00:00.000000Z"}]}"#;
        let msg = Message::new("kraken", trade_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.kraken.json.trade.BTC-USD");
        assert_eq!(received.payload.as_ref(), trade_json);
    }

    #[tokio::test]
    async fn test_publish_ticker_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenNatsWriter::new(transport.clone(), "dev", "kraken");

        let mut sub = transport
            .subscribe("dev.kraken.json.ticker.BTC-USD")
            .await
            .unwrap();

        let ticker_json = br#"{"channel":"ticker","type":"update","data":[{"symbol":"BTC/USD","bid":97000.0,"bid_qty":0.50000000,"ask":97000.1,"ask_qty":1.00000000,"last":97000.0,"volume":1234.56789012,"vwap":96500.0,"low":95000.0,"high":98000.0,"change":500.0,"change_pct":0.52}]}"#;
        let msg = Message::new("kraken", ticker_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.kraken.json.ticker.BTC-USD");
        assert_eq!(received.payload.as_ref(), ticker_json);
    }

    #[tokio::test]
    async fn test_skip_heartbeat() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenNatsWriter::new(transport.clone(), "dev", "kraken");

        let heartbeat_json = br#"{"channel":"heartbeat","type":"update"}"#;
        let msg = Message::new("kraken", heartbeat_json.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_skip_pong() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenNatsWriter::new(transport.clone(), "dev", "kraken");

        let pong_json = br#"{"method":"pong","time_in":"2026-02-06T12:00:00.000000Z","time_out":"2026-02-06T12:00:00.000001Z"}"#;
        let msg = Message::new("kraken", pong_json.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_skip_subscription_result() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenNatsWriter::new(transport.clone(), "dev", "kraken");

        let sub_json = br#"{"method":"subscribe","result":{"channel":"ticker","symbol":"BTC/USD"},"success":true,"time_in":"2026-02-06T12:00:00.000000Z","time_out":"2026-02-06T12:00:00.000001Z"}"#;
        let msg = Message::new("kraken", sub_json.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_message_count() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenNatsWriter::new(transport.clone(), "dev", "kraken");

        let _sub = transport
            .subscribe("dev.kraken.json.trade.BTC-USD")
            .await
            .unwrap();

        let trade_json = br#"{"channel":"trade","type":"update","data":[{"symbol":"BTC/USD","side":"buy","price":97000.0,"qty":0.001,"ord_type":"market","trade_id":"12345","timestamp":"2026-02-06T12:00:00.000000Z"}]}"#;
        let msg = Message::new("kraken", trade_json.to_vec());

        writer.write(&msg).await.unwrap();
        writer.write(&msg).await.unwrap();

        assert_eq!(writer.message_count(), 2);
    }

    #[tokio::test]
    async fn test_with_prefix() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer =
            KrakenNatsWriter::with_prefix(transport.clone(), "prod.kraken.main", "PROD_KRAKEN");

        let mut sub = transport
            .subscribe("prod.kraken.main.json.ticker.ETH-USD")
            .await
            .unwrap();

        let ticker_json = br#"{"channel":"ticker","type":"update","data":[{"symbol":"ETH/USD","bid":3200.0,"bid_qty":10.0,"ask":3201.0,"ask_qty":5.0,"last":3200.5,"volume":50000.0,"vwap":3180.0,"low":3100.0,"high":3300.0,"change":50.0,"change_pct":1.5}]}"#;
        let msg = Message::new("kraken", ticker_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "prod.kraken.main.json.ticker.ETH-USD");
    }
}
