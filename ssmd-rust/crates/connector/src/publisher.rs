//! Publisher for sending normalized data to transport
//!
//! Optimized for low-latency publishing:
//! - Thread-local buffer pool avoids allocations per message
//! - Subject caching via SubjectBuilder avoids format! per publish
//! - Arc<str> for subject strings (cheap clone)

use std::cell::RefCell;
use std::sync::Arc;

use bytes::Bytes;
use capnp::message::Builder;
use ssmd_middleware::{SubjectBuilder, Transport, TransportError};
use ssmd_schema::{trade, Side};

/// Initial capacity for serialization buffer (typical trade message ~100 bytes)
const BUFFER_INITIAL_CAPACITY: usize = 256;

thread_local! {
    /// Thread-local buffer for Cap'n Proto serialization.
    /// Reused across publish calls to avoid heap allocations.
    static CAPNP_BUFFER: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(BUFFER_INITIAL_CAPACITY));
}

/// Trade data for publishing
#[derive(Debug, Clone)]
pub struct TradeData {
    pub timestamp_nanos: u64,
    pub ticker: String,
    pub price: f64,
    pub size: u32,
    pub side: TradeSide,
    pub trade_id: String,
}

#[derive(Debug, Clone, Copy)]
pub enum TradeSide {
    Buy,
    Sell,
}

/// Publisher for sending Cap'n Proto encoded messages to transport.
/// Uses thread-local buffer pool and subject caching for low latency.
pub struct Publisher {
    transport: Arc<dyn Transport>,
    subjects: SubjectBuilder,
}

impl Publisher {
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<Arc<str>>,
        feed_name: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::new(env_name, feed_name),
        }
    }

    /// Publish a trade to the transport.
    /// Uses thread-local buffer to avoid allocation per message.
    pub async fn publish_trade(&self, trade_data: &TradeData) -> Result<(), TransportError> {
        // Get cached subject (no allocation after first call per ticker)
        let subject = self.subjects.trade(&trade_data.ticker);

        // Serialize using thread-local buffer
        let payload = CAPNP_BUFFER.with(|buf| {
            let mut buffer = buf.borrow_mut();
            buffer.clear();

            // Build Cap'n Proto message
            let mut message = Builder::new_default();
            {
                let mut trade_builder = message.init_root::<trade::Builder>();
                trade_builder.set_timestamp(trade_data.timestamp_nanos);
                trade_builder.set_ticker(&trade_data.ticker);
                trade_builder.set_price(trade_data.price);
                trade_builder.set_size(trade_data.size);
                trade_builder.set_side(match trade_data.side {
                    TradeSide::Buy => Side::Buy,
                    TradeSide::Sell => Side::Sell,
                });
                trade_builder.set_trade_id(&trade_data.trade_id);
            }

            // Serialize to thread-local buffer
            capnp::serialize::write_message(&mut *buffer, &message)
                .map_err(|e| TransportError::PublishFailed(e.to_string()))?;

            // Copy to Bytes (single allocation, necessary for async send)
            Ok::<_, TransportError>(Bytes::copy_from_slice(&buffer))
        })?;

        // Publish to transport
        self.transport.publish(&subject, payload).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_middleware::InMemoryTransport;

    #[tokio::test]
    async fn test_publish_trade() {
        let transport = Arc::new(InMemoryTransport::new());
        let publisher = Publisher::new(transport.clone(), "kalshi-dev", "kalshi");

        // Subscribe before publishing
        let mut sub = transport.subscribe("kalshi-dev.kalshi.trade.BTCUSD").await.unwrap();

        let trade = TradeData {
            timestamp_nanos: 1703318400000000000,
            ticker: "BTCUSD".to_string(),
            price: 100.50,
            size: 10,
            side: TradeSide::Buy,
            trade_id: "trade-001".to_string(),
        };

        publisher.publish_trade(&trade).await.unwrap();

        // Receive and verify
        let msg = sub.next().await.unwrap();
        assert_eq!(msg.subject, "kalshi-dev.kalshi.trade.BTCUSD");
        assert!(!msg.payload.is_empty());

        // Deserialize and verify
        let reader = capnp::serialize::read_message_from_flat_slice(
            &mut msg.payload.as_ref(),
            capnp::message::ReaderOptions::new(),
        )
        .unwrap();
        let trade_reader = reader.get_root::<trade::Reader>().unwrap();
        assert_eq!(trade_reader.get_ticker().unwrap(), "BTCUSD");
        assert_eq!(trade_reader.get_price(), 100.50);
    }
}
