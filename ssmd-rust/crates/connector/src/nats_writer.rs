//! NATS Writer - publishes Cap'n Proto encoded messages to NATS
//!
//! Parses incoming JSON messages from connectors, converts to Cap'n Proto,
//! and publishes to appropriate NATS subjects.
//!
//! TODO: Add raw JSON capture path alongside Cap'n Proto normalization

use std::cell::RefCell;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use capnp::message::Builder;
use tracing::{trace, warn};

use ssmd_middleware::{SubjectBuilder, Transport, TransportError};
use ssmd_schema::{ticker, trade, Side};

use crate::error::WriterError;
use crate::kalshi::messages::{TickerData, TradeData, WsMessage};
use crate::message::Message;
use crate::traits::Writer;

/// Initial capacity for serialization buffer
const BUFFER_CAPACITY: usize = 256;

thread_local! {
    /// Thread-local buffer for Cap'n Proto serialization
    static CAPNP_BUFFER: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(BUFFER_CAPACITY));
}

/// Writer that publishes Cap'n Proto encoded messages to NATS
pub struct NatsWriter {
    transport: Arc<dyn Transport>,
    subjects: SubjectBuilder,
    trade_count: u64,
    ticker_count: u64,
}

impl NatsWriter {
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<String>,
        feed_name: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::new(env_name, feed_name),
            trade_count: 0,
            ticker_count: 0,
        }
    }

    /// Publish a trade to NATS
    async fn publish_trade(&mut self, data: &TradeData) -> Result<(), TransportError> {
        let subject = self.subjects.trade(&data.market_ticker);

        let payload = CAPNP_BUFFER.with(|buf| {
            let mut buffer = buf.borrow_mut();
            buffer.clear();

            let mut message = Builder::new_default();
            {
                let mut trade_builder = message.init_root::<trade::Builder>();
                trade_builder.set_timestamp(data.ts.timestamp_nanos_opt().unwrap_or(0) as u64);
                trade_builder.set_ticker(&data.market_ticker);
                // Kalshi prices are in cents (0-100), convert to decimal
                trade_builder.set_price(data.price as f64 / 100.0);
                trade_builder.set_size(data.count as u32);
                trade_builder.set_side(match data.side.as_str() {
                    "yes" | "buy" => Side::Buy,
                    _ => Side::Sell,
                });
                trade_builder.set_trade_id(""); // Kalshi doesn't include trade_id in WS
            }

            capnp::serialize::write_message(&mut *buffer, &message)
                .map_err(|e| TransportError::PublishFailed(e.to_string()))?;

            Ok::<_, TransportError>(Bytes::copy_from_slice(&buffer))
        })?;

        self.transport.publish(&subject, payload).await?;
        self.trade_count += 1;
        Ok(())
    }

    /// Publish a ticker update to NATS
    async fn publish_ticker(&mut self, data: &TickerData) -> Result<(), TransportError> {
        let subject = self.subjects.ticker(&data.market_ticker);

        let payload = CAPNP_BUFFER.with(|buf| {
            let mut buffer = buf.borrow_mut();
            buffer.clear();

            let mut message = Builder::new_default();
            {
                let mut ticker_builder = message.init_root::<ticker::Builder>();
                ticker_builder.set_timestamp(data.ts.timestamp_nanos_opt().unwrap_or(0) as u64);
                ticker_builder.set_ticker(&data.market_ticker);
                // Kalshi prices are in cents (0-100), convert to decimal
                ticker_builder.set_bid_price(data.yes_bid.unwrap_or(0) as f64 / 100.0);
                ticker_builder.set_ask_price(data.yes_ask.unwrap_or(0) as f64 / 100.0);
                ticker_builder.set_last_price(data.last_price.unwrap_or(0) as f64 / 100.0);
                ticker_builder.set_volume(data.volume.unwrap_or(0) as u64);
                ticker_builder.set_open_interest(data.open_interest.unwrap_or(0) as u64);
            }

            capnp::serialize::write_message(&mut *buffer, &message)
                .map_err(|e| TransportError::PublishFailed(e.to_string()))?;

            Ok::<_, TransportError>(Bytes::copy_from_slice(&buffer))
        })?;

        self.transport.publish(&subject, payload).await?;
        self.ticker_count += 1;
        Ok(())
    }

    /// Get count of published trades
    pub fn trade_count(&self) -> u64 {
        self.trade_count
    }

    /// Get count of published tickers
    pub fn ticker_count(&self) -> u64 {
        self.ticker_count
    }
}

#[async_trait]
impl Writer for NatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        // Parse JSON to determine message type
        let ws_msg: WsMessage = match serde_json::from_slice(&msg.data) {
            Ok(m) => m,
            Err(e) => {
                trace!(error = %e, "Failed to parse message, skipping");
                return Ok(()); // Skip unparseable messages
            }
        };

        match ws_msg {
            WsMessage::Trade { msg: trade_data } => {
                self.publish_trade(&trade_data).await.map_err(|e| {
                    WriterError::WriteFailed(format!("NATS publish failed: {}", e))
                })?;
            }
            WsMessage::Ticker { msg: ticker_data } => {
                self.publish_ticker(&ticker_data).await.map_err(|e| {
                    WriterError::WriteFailed(format!("NATS publish failed: {}", e))
                })?;
            }
            WsMessage::OrderbookSnapshot { .. } | WsMessage::OrderbookDelta { .. } => {
                // TODO: Publish orderbook updates when L2 support is added
                trace!("Orderbook message received, skipping (not yet implemented)");
            }
            WsMessage::Subscribed { .. } | WsMessage::Unsubscribed { .. } => {
                // Control messages, don't publish
            }
            WsMessage::Unknown => {
                warn!("Unknown message type received");
            }
        }

        Ok(())
    }

    async fn close(&mut self) -> Result<(), WriterError> {
        trace!(
            trades = self.trade_count,
            tickers = self.ticker_count,
            "NatsWriter closing"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_middleware::InMemoryTransport;

    #[tokio::test]
    async fn test_publish_trade() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        // Subscribe before publishing (exact subject - InMemoryTransport doesn't support wildcards)
        let mut sub = transport.subscribe("dev.kalshi.trade.KXTEST-123").await.unwrap();

        let trade_json = br#"{"type":"trade","sid":2,"seq":1,"msg":{"market_ticker":"KXTEST-123","price":50,"count":10,"side":"yes","ts":1732579880}}"#;
        let msg = Message::new("kalshi", trade_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.kalshi.trade.KXTEST-123");
        assert!(!received.payload.is_empty());

        // Deserialize and verify
        let reader = capnp::serialize::read_message_from_flat_slice(
            &mut received.payload.as_ref(),
            capnp::message::ReaderOptions::new(),
        )
        .unwrap();
        let trade_reader = reader.get_root::<trade::Reader>().unwrap();
        assert_eq!(trade_reader.get_ticker().unwrap(), "KXTEST-123");
        assert_eq!(trade_reader.get_price(), 0.50); // 50 cents = 0.50
        assert_eq!(trade_reader.get_size(), 10);
    }

    #[tokio::test]
    async fn test_publish_ticker() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        // Subscribe to exact subject - InMemoryTransport doesn't support wildcards
        let mut sub = transport.subscribe("dev.kalshi.ticker.KXTEST-456").await.unwrap();

        let ticker_json = br#"{"type":"ticker","sid":1,"msg":{"market_ticker":"KXTEST-456","yes_bid":45,"yes_ask":46,"price":45,"volume":1000,"open_interest":500,"ts":1732579880}}"#;
        let msg = Message::new("kalshi", ticker_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.kalshi.ticker.KXTEST-456");

        let reader = capnp::serialize::read_message_from_flat_slice(
            &mut received.payload.as_ref(),
            capnp::message::ReaderOptions::new(),
        )
        .unwrap();
        let ticker_reader = reader.get_root::<ticker::Reader>().unwrap();
        assert_eq!(ticker_reader.get_ticker().unwrap(), "KXTEST-456");
        assert_eq!(ticker_reader.get_bid_price(), 0.45);
        assert_eq!(ticker_reader.get_ask_price(), 0.46);
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
        assert_eq!(writer.trade_count(), 0);
        assert_eq!(writer.ticker_count(), 0);
    }
}
