//! Polymarket NATS Writer - publishes raw JSON messages to NATS
//!
//! Routes Polymarket market data messages to appropriate NATS subjects.
//! Uses condition_id (market field) for subject routing — shorter than token IDs
//! and naturally groups Yes/No token data for the same market.
//! Passes through raw bytes - no transformation.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use tracing::trace;

use ssmd_middleware::{sanitize_subject_token, SubjectBuilder, Transport};

use crate::error::WriterError;
use crate::message::Message;
use crate::polymarket::messages::PolymarketWsMessage;
use crate::traits::Writer;

/// Writer that publishes raw Polymarket JSON messages to NATS
pub struct PolymarketNatsWriter {
    transport: Arc<dyn Transport>,
    subjects: SubjectBuilder,
    message_count: u64,
}

impl PolymarketNatsWriter {
    /// Create a new PolymarketNatsWriter with default subject prefix: {env_name}.{feed_name}
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<String>,
        feed_name: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::new(env_name, feed_name),
            message_count: 0,
        }
    }

    /// Create a new PolymarketNatsWriter with a custom subject prefix and stream name.
    pub fn with_prefix(
        transport: Arc<dyn Transport>,
        subject_prefix: impl Into<String>,
        stream_name: impl Into<String>,
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

#[async_trait]
impl Writer for PolymarketNatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        let ws_msg: PolymarketWsMessage = match serde_json::from_slice(&msg.data) {
            Ok(m) => m,
            Err(e) => {
                let preview: String = String::from_utf8_lossy(&msg.data)
                    .chars()
                    .take(500)
                    .collect();

                // Skip PONG responses and other non-JSON messages silently
                if preview == "PONG" || preview.starts_with("PONG") {
                    return Ok(());
                }

                return Err(WriterError::WriteFailed(format!(
                    "Failed to parse Polymarket message: {}. Preview: {}",
                    e, preview
                )));
            }
        };

        let condition_id = sanitize_subject_token(ws_msg.condition_id());

        let subject = match &ws_msg {
            PolymarketWsMessage::LastTradePrice { .. } => {
                self.subjects.json_trade(&condition_id)
            }
            PolymarketWsMessage::PriceChange { .. } => {
                self.subjects.json_ticker(&condition_id)
            }
            PolymarketWsMessage::Book { .. } => {
                self.subjects.json_orderbook(&condition_id)
            }
            PolymarketWsMessage::BestBidAsk { .. } => {
                self.subjects.json_ticker(&condition_id)
            }
            PolymarketWsMessage::NewMarket { .. }
            | PolymarketWsMessage::MarketResolved { .. } => {
                self.subjects.json_lifecycle(&condition_id)
            }
            PolymarketWsMessage::TickSizeChange { .. } => {
                // Tick size changes are operational - log and skip
                trace!(market = %condition_id, "Tick size change event, skipping publish");
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
        trace!(messages = self.message_count, "PolymarketNatsWriter closing");
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
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let mut sub = transport
            .subscribe("dev.polymarket.json.trade.0x1234abcd")
            .await
            .unwrap();

        let trade_json = br#"{"event_type":"last_trade_price","asset_id":"token123","market":"0x1234abcd","price":"0.55","side":"BUY","size":"100","timestamp":"1706000000000"}"#;
        let msg = Message::new("polymarket", trade_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.polymarket.json.trade.0x1234abcd");
        assert_eq!(received.payload.as_ref(), trade_json);
    }

    #[tokio::test]
    async fn test_publish_ticker_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let mut sub = transport
            .subscribe("dev.polymarket.json.ticker.0x1234abcd")
            .await
            .unwrap();

        let price_change_json = br#"{"event_type":"price_change","market":"0x1234abcd","timestamp":"1706000000000","price_changes":[{"asset_id":"token123","price":"0.55","size":"750","side":"BUY"}]}"#;
        let msg = Message::new("polymarket", price_change_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.polymarket.json.ticker.0x1234abcd");
    }

    #[tokio::test]
    async fn test_publish_orderbook_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let mut sub = transport
            .subscribe("dev.polymarket.json.orderbook.0x1234abcd")
            .await
            .unwrap();

        let book_json = br#"{"event_type":"book","asset_id":"token123","market":"0x1234abcd","buys":[{"price":"0.55","size":"1000"}],"sells":[{"price":"0.56","size":"500"}]}"#;
        let msg = Message::new("polymarket", book_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(
            received.subject,
            "dev.polymarket.json.orderbook.0x1234abcd"
        );
    }

    #[tokio::test]
    async fn test_publish_lifecycle_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let mut sub = transport
            .subscribe("dev.polymarket.json.lifecycle.0x1234abcd")
            .await
            .unwrap();

        let resolved_json = br#"{"event_type":"market_resolved","market":"0x1234abcd","winning_outcome":"Yes","timestamp":"1706000000000"}"#;
        let msg = Message::new("polymarket", resolved_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(
            received.subject,
            "dev.polymarket.json.lifecycle.0x1234abcd"
        );
    }

    #[tokio::test]
    async fn test_skip_pong() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let pong_msg = Message::new("polymarket", b"PONG".to_vec());
        writer.write(&pong_msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_skip_tick_size_change() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let tick_json = br#"{"event_type":"tick_size_change","asset_id":"token123","market":"0x1234abcd","old_tick_size":"0.01","new_tick_size":"0.001","side":"BUY","timestamp":"1706000000000"}"#;
        let msg = Message::new("polymarket", tick_json.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_message_count() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let _sub = transport
            .subscribe("dev.polymarket.json.trade.0xabc")
            .await
            .unwrap();

        let trade_json =
            br#"{"event_type":"last_trade_price","asset_id":"t1","market":"0xabc","price":"0.50"}"#;
        let msg = Message::new("polymarket", trade_json.to_vec());

        writer.write(&msg).await.unwrap();
        writer.write(&msg).await.unwrap();

        assert_eq!(writer.message_count(), 2);
    }

    #[tokio::test]
    async fn test_with_prefix() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::with_prefix(
            transport.clone(),
            "prod.polymarket",
            "PROD_POLYMARKET",
        );

        let mut sub = transport
            .subscribe("prod.polymarket.json.trade.0xdef")
            .await
            .unwrap();

        let trade_json =
            br#"{"event_type":"last_trade_price","asset_id":"t1","market":"0xdef","price":"0.75"}"#;
        let msg = Message::new("polymarket", trade_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "prod.polymarket.json.trade.0xdef");
    }
}
