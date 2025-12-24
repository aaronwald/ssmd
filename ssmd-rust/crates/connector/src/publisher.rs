//! Publisher for sending normalized data to transport

use std::sync::Arc;

use bytes::Bytes;
use capnp::message::Builder;
use ssmd_middleware::{Transport, TransportError};
use ssmd_schema::{order_book_update, trade, Side};

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

/// Order book level data
#[derive(Debug, Clone)]
pub struct LevelData {
    pub price: f64,
    pub size: u32,
}

/// Order book update data for publishing
#[derive(Debug, Clone)]
pub struct OrderBookData {
    pub timestamp_nanos: u64,
    pub ticker: String,
    pub bids: Vec<LevelData>,
    pub asks: Vec<LevelData>,
}

/// Publisher for sending Cap'n Proto encoded messages to transport
pub struct Publisher {
    transport: Arc<dyn Transport>,
    env_prefix: String,
    feed_name: String,
}

impl Publisher {
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<String>,
        feed_name: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            env_prefix: env_name.into(),
            feed_name: feed_name.into(),
        }
    }

    /// Publish a trade to the transport
    pub async fn publish_trade(&self, trade_data: &TradeData) -> Result<(), TransportError> {
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

        // Serialize to bytes
        let mut output = Vec::new();
        capnp::serialize::write_message(&mut output, &message)
            .map_err(|e| TransportError::PublishFailed(e.to_string()))?;

        // Publish to transport
        let subject = format!(
            "{}.{}.trade.{}",
            self.env_prefix, self.feed_name, trade_data.ticker
        );
        self.transport.publish(&subject, Bytes::from(output)).await
    }

    /// Publish an orderbook update to the transport
    pub async fn publish_orderbook(&self, book: &OrderBookData) -> Result<(), TransportError> {
        let mut message = Builder::new_default();
        {
            let mut book_builder = message.init_root::<order_book_update::Builder>();
            book_builder.set_timestamp(book.timestamp_nanos);
            book_builder.set_ticker(&book.ticker);

            let mut bids = book_builder.reborrow().init_bids(book.bids.len() as u32);
            for (i, bid) in book.bids.iter().enumerate() {
                let mut level_builder = bids.reborrow().get(i as u32);
                level_builder.set_price(bid.price);
                level_builder.set_size(bid.size);
            }

            let mut asks = book_builder.reborrow().init_asks(book.asks.len() as u32);
            for (i, ask) in book.asks.iter().enumerate() {
                let mut level_builder = asks.reborrow().get(i as u32);
                level_builder.set_price(ask.price);
                level_builder.set_size(ask.size);
            }
        }

        let mut output = Vec::new();
        capnp::serialize::write_message(&mut output, &message)
            .map_err(|e| TransportError::PublishFailed(e.to_string()))?;

        let subject = format!(
            "{}.{}.orderbook.{}",
            self.env_prefix, self.feed_name, book.ticker
        );
        self.transport.publish(&subject, Bytes::from(output)).await
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

    #[tokio::test]
    async fn test_publish_orderbook() {
        let transport = Arc::new(InMemoryTransport::new());
        let publisher = Publisher::new(transport.clone(), "kalshi-dev", "kalshi");

        let mut sub = transport.subscribe("kalshi-dev.kalshi.orderbook.BTCUSD").await.unwrap();

        let book = OrderBookData {
            timestamp_nanos: 1703318400000000000,
            ticker: "BTCUSD".to_string(),
            bids: vec![LevelData { price: 100.0, size: 10 }],
            asks: vec![LevelData { price: 101.0, size: 5 }],
        };

        publisher.publish_orderbook(&book).await.unwrap();

        let msg = sub.next().await.unwrap();
        assert_eq!(msg.subject, "kalshi-dev.kalshi.orderbook.BTCUSD");
        assert!(!msg.payload.is_empty());
    }
}
