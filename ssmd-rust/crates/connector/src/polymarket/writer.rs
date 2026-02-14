//! Polymarket NATS Writer - publishes raw JSON messages to NATS
//!
//! Routes Polymarket market data messages to appropriate NATS subjects.
//! Uses condition_id (market field) for subject routing — shorter than token IDs
//! and naturally groups Yes/No token data for the same market.
//! Passes through raw bytes - no transformation.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use tracing::{trace, warn};

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

    fn subject_from_element(&self, element: &serde_json::Value) -> Option<String> {
        let market = element
            .get("market")
            .and_then(|v| v.as_str())
            .map(sanitize_subject_token)?;

        match element.get("event_type").and_then(|v| v.as_str()) {
            Some("last_trade_price") => Some(self.subjects.json_trade(&market)),
            Some("price_change") | Some("best_bid_ask") => Some(self.subjects.json_ticker(&market)),
            Some("book") => Some(self.subjects.json_orderbook(&market)),
            Some("new_market") | Some("market_resolved") => Some(self.subjects.json_lifecycle(&market)),
            Some("tick_size_change") => None,
            Some(_) => None,
            None => {
                if element.get("bids").is_some() || element.get("asks").is_some() {
                    Some(self.subjects.json_orderbook(&market))
                } else {
                    None
                }
            }
        }
    }
}

#[async_trait]
impl Writer for PolymarketNatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        let preview_fn = || -> String {
            String::from_utf8_lossy(&msg.data)
                .chars()
                .take(500)
                .collect()
        };

        // Skip PONG responses
        if msg.data.as_ref() == b"PONG" {
            return Ok(());
        }

        // Fast path: most payloads are single typed JSON objects. Route and publish
        // raw bytes directly to avoid JSON re-serialization and extra allocation.
        if let Ok(ws_msg) = serde_json::from_slice::<PolymarketWsMessage>(&msg.data) {
            let condition_id = sanitize_subject_token(ws_msg.condition_id());

            let subject = match ws_msg {
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
                    trace!(market = %condition_id, "Tick size change event, skipping publish");
                    return Ok(());
                }
            };

            self.transport
                .publish(&subject, msg.data.clone())
                .await
                .map_err(|e| WriterError::WriteFailed(format!("NATS publish failed: {}", e)))?;

            self.message_count += 1;
            return Ok(());
        }

        // Polymarket sends all messages as JSON arrays: [{...}, {...}]
        // Parse as array of raw values, then process each element individually.
        let elements: Vec<serde_json::Value> = match serde_json::from_slice(&msg.data) {
            Ok(serde_json::Value::Array(arr)) => arr,
            Ok(obj @ serde_json::Value::Object(_)) => vec![obj],
            Ok(_) => {
                warn!(preview = %preview_fn(), "Unexpected JSON type from Polymarket");
                return Ok(());
            }
            Err(e) => {
                return Err(WriterError::WriteFailed(format!(
                    "Failed to parse Polymarket message: {}. Preview: {}",
                    e,
                    preview_fn()
                )));
            }
        };

        for element in elements {
            let Some(subject) = self.subject_from_element(&element) else {
                trace!(preview = %element, "Skipping unsupported Polymarket element");
                continue;
            };

            // Publish individual element as raw JSON bytes
            let element_bytes = serde_json::to_vec(&element)
                .map_err(|e| WriterError::WriteFailed(format!("JSON serialize failed: {}", e)))?;
            self.transport
                .publish(&subject, Bytes::from(element_bytes))
                .await
                .map_err(|e| WriterError::WriteFailed(format!("NATS publish failed: {}", e)))?;

            self.message_count += 1;
        }

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
        // Compare parsed JSON (key order may differ after re-serialization)
        let expected: serde_json::Value = serde_json::from_slice(trade_json).unwrap();
        let actual: serde_json::Value = serde_json::from_slice(&received.payload).unwrap();
        assert_eq!(actual, expected);
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

        let book_json = br#"{"event_type":"book","asset_id":"token123","market":"0x1234abcd","bids":[{"price":"0.55","size":"1000"}],"asks":[{"price":"0.56","size":"500"}]}"#;
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
    async fn test_publish_best_bid_ask_routes_to_ticker() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let mut sub = transport
            .subscribe("dev.polymarket.json.ticker.0x1234abcd")
            .await
            .unwrap();

        let bba_json = br#"{"event_type":"best_bid_ask","market":"0x1234abcd","asset_id":"token123","best_bid":"0.54","best_ask":"0.56"}"#;
        let msg = Message::new("polymarket", bba_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.polymarket.json.ticker.0x1234abcd");
        assert_eq!(writer.message_count(), 1);
    }

    #[tokio::test]
    async fn test_publish_new_market_routes_to_lifecycle() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let mut sub = transport
            .subscribe("dev.polymarket.json.lifecycle.0x1234abcd")
            .await
            .unwrap();

        let new_market_json = br#"{"event_type":"new_market","market":"0x1234abcd","assets_ids":["token_yes","token_no"],"question":"Will X happen?","outcomes":["Yes","No"]}"#;
        let msg = Message::new("polymarket", new_market_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(
            received.subject,
            "dev.polymarket.json.lifecycle.0x1234abcd"
        );
        assert_eq!(writer.message_count(), 1);
    }

    #[tokio::test]
    async fn test_invalid_json_returns_error() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let garbage = Message::new("polymarket", b"not valid json at all".to_vec());
        let result = writer.write(&garbage).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Failed to parse Polymarket message"),
            "Expected parse error, got: {}",
            err
        );
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_array_wrapped_message() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let mut sub = transport
            .subscribe("dev.polymarket.json.trade.0x1234abcd")
            .await
            .unwrap();

        // Polymarket sends messages wrapped in arrays
        let array_json = br#"[{"event_type":"last_trade_price","asset_id":"token123","market":"0x1234abcd","price":"0.55"}]"#;
        let msg = Message::new("polymarket", array_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.polymarket.json.trade.0x1234abcd");
        assert_eq!(writer.message_count(), 1);
    }

    #[tokio::test]
    async fn test_book_snapshot_without_event_type() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = PolymarketNatsWriter::new(transport.clone(), "dev", "polymarket");

        let mut sub = transport
            .subscribe("dev.polymarket.json.orderbook.0x1234abcd")
            .await
            .unwrap();

        // Book snapshots from WS don't have event_type
        let book_json = br#"[{"asset_id":"token123","market":"0x1234abcd","timestamp":"1706000000000","hash":"abc123","bids":[{"price":"0.55","size":"1000"}],"asks":[{"price":"0.56","size":"500"}]}]"#;
        let msg = Message::new("polymarket", book_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(
            received.subject,
            "dev.polymarket.json.orderbook.0x1234abcd"
        );
        assert_eq!(writer.message_count(), 1);
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
