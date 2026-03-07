//! Price feed abstraction for PriceMonitor.
//!
//! Provides a trait for consuming live market prices and a NATS
//! implementation that subscribes to Kalshi ticker subjects.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use tokio::sync::broadcast;

/// A single price update for a market ticker.
#[derive(Debug, Clone)]
pub struct PriceTick {
    pub ticker: String,
    pub yes_bid: Option<i64>, // cents (1-99)
    pub yes_ask: Option<i64>, // cents (1-99)
    pub last_price: Option<i64>,
    pub ts: DateTime<Utc>,
}

impl PriceTick {
    /// Get yes_bid as dollars (e.g., 45 cents → 0.45)
    pub fn yes_bid_dollars(&self) -> Option<Decimal> {
        self.yes_bid.map(|c| Decimal::new(c, 2))
    }

    /// Get yes_ask as dollars (e.g., 55 cents → 0.55)
    pub fn yes_ask_dollars(&self) -> Option<Decimal> {
        self.yes_ask.map(|c| Decimal::new(c, 2))
    }
}

/// Trait for receiving live price data.
/// Implementations: NatsPriceFeed (prod), MockPriceFeed (test).
#[async_trait]
pub trait PriceFeed: Send + Sync {
    /// Subscribe to price updates. Returns a broadcast receiver.
    /// All tickers arrive on the same channel — caller filters by ticker.
    fn subscribe(&self) -> broadcast::Receiver<PriceTick>;

    /// Check if the feed is connected/healthy.
    fn is_connected(&self) -> bool;
}

// ---------------------------------------------------------------------------
// NATS implementation
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use tracing::{error, trace};

/// NATS-based price feed that subscribes to Kalshi ticker subjects.
///
/// Subscribes to plain NATS subjects (not JetStream) since PriceMonitor
/// only needs real-time data, not replay.
pub struct NatsPriceFeed {
    sender: broadcast::Sender<PriceTick>,
    connected: Arc<AtomicBool>,
}

impl NatsPriceFeed {
    /// Connect to NATS and start consuming ticker messages.
    ///
    /// `subjects` should be NATS wildcard subjects for ticker data, e.g.:
    /// `["prod.kalshi.crypto.json.ticker.>"]`
    ///
    /// Each subject gets its own background task that parses messages and
    /// broadcasts PriceTicks on a shared channel.
    pub async fn connect(nats_url: &str, subjects: &[String]) -> Result<Self, String> {
        let client = async_nats::connect(nats_url)
            .await
            .map_err(|e| format!("NATS connect failed: {}", e))?;

        let (sender, _) = broadcast::channel(4096);
        let connected = Arc::new(AtomicBool::new(true));

        for subject in subjects {
            let subscriber = client
                .subscribe(subject.clone())
                .await
                .map_err(|e| format!("NATS subscribe to {} failed: {}", subject, e))?;

            let tx = sender.clone();
            let conn = connected.clone();
            let subj = subject.clone();

            tokio::spawn(async move {
                Self::consume_loop(subscriber, tx, conn, &subj).await;
            });
        }

        Ok(Self { sender, connected })
    }

    async fn consume_loop(
        mut subscriber: async_nats::Subscriber,
        sender: broadcast::Sender<PriceTick>,
        connected: Arc<AtomicBool>,
        subject: &str,
    ) {
        while let Some(msg) = subscriber.next().await {
            let tick = match Self::parse_ticker_message(&msg.payload) {
                Some(t) => t,
                None => continue,
            };

            trace!(
                ticker = %tick.ticker,
                yes_bid = ?tick.yes_bid,
                yes_ask = ?tick.yes_ask,
                "price tick"
            );

            // Broadcast — if no receivers, that's fine
            let _ = sender.send(tick);
        }

        // NATS subscription ended — mark disconnected
        connected.store(false, Ordering::SeqCst);
        error!(subject, "NATS price feed subscription ended — PriceMonitor degraded");
    }

    fn parse_ticker_message(payload: &[u8]) -> Option<PriceTick> {
        // Raw envelope: {"type":"ticker","sid":1,"msg":{"market_ticker":"...","yes_bid":N,...,"ts":N}}
        let envelope: serde_json::Value = serde_json::from_slice(payload).ok()?;

        // Only process ticker messages (skip trade, lifecycle, etc.)
        let msg_type = envelope.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if msg_type != "ticker" {
            return None;
        }

        let msg = envelope.get("msg")?;

        let ticker = msg.get("market_ticker")?.as_str()?.to_string();
        let yes_bid = msg.get("yes_bid").and_then(|v| v.as_i64());
        let yes_ask = msg.get("yes_ask").and_then(|v| v.as_i64());
        let last_price = msg
            .get("last_price")
            .or_else(|| msg.get("price"))
            .and_then(|v| v.as_i64());
        let ts_secs = msg.get("ts").and_then(|v| v.as_i64())?;
        let ts = DateTime::from_timestamp(ts_secs, 0)?;

        Some(PriceTick {
            ticker,
            yes_bid,
            yes_ask,
            last_price,
            ts,
        })
    }
}

#[async_trait]
impl PriceFeed for NatsPriceFeed {
    fn subscribe(&self) -> broadcast::Receiver<PriceTick> {
        self.sender.subscribe()
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Mock for testing
// ---------------------------------------------------------------------------

/// Test mock: allows pushing prices programmatically.
#[cfg(test)]
pub struct MockPriceFeed {
    sender: broadcast::Sender<PriceTick>,
}

#[cfg(test)]
impl MockPriceFeed {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self { sender }
    }

    /// Push a price tick (for test driving).
    pub fn send_tick(&self, tick: PriceTick) {
        let _ = self.sender.send(tick);
    }
}

#[cfg(test)]
#[async_trait]
impl PriceFeed for MockPriceFeed {
    fn subscribe(&self) -> broadcast::Receiver<PriceTick> {
        self.sender.subscribe()
    }

    fn is_connected(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ticker_message() {
        let payload = br#"{"type":"ticker","sid":1,"msg":{"market_id":"abc","market_ticker":"KXBTCD-26MAR0211","price":50,"yes_bid":48,"yes_ask":52,"price_dollars":"0.5000","yes_bid_dollars":"0.4800","yes_ask_dollars":"0.5200","volume":1000,"open_interest":500,"dollar_volume":500,"dollar_open_interest":250,"ts":1732579880,"Clock":123}}"#;
        let tick = NatsPriceFeed::parse_ticker_message(payload).unwrap();
        assert_eq!(tick.ticker, "KXBTCD-26MAR0211");
        assert_eq!(tick.yes_bid, Some(48));
        assert_eq!(tick.yes_ask, Some(52));
        assert_eq!(tick.last_price, Some(50));
        assert_eq!(tick.ts.timestamp(), 1732579880);
    }

    #[test]
    fn test_parse_ticker_message_minimal() {
        let payload = br#"{"type":"ticker","sid":1,"msg":{"market_ticker":"KXTEST","ts":1732579880}}"#;
        let tick = NatsPriceFeed::parse_ticker_message(payload).unwrap();
        assert_eq!(tick.ticker, "KXTEST");
        assert_eq!(tick.yes_bid, None);
        assert_eq!(tick.yes_ask, None);
    }

    #[test]
    fn test_parse_trade_message_returns_none() {
        let payload = br#"{"type":"trade","sid":1,"msg":{"market_ticker":"KXTEST","price":50,"count":10,"side":"yes","ts":1732579880}}"#;
        assert!(NatsPriceFeed::parse_ticker_message(payload).is_none());
    }

    #[test]
    fn test_parse_invalid_json_returns_none() {
        assert!(NatsPriceFeed::parse_ticker_message(b"not json").is_none());
    }

    #[test]
    fn test_yes_bid_dollars() {
        let tick = PriceTick {
            ticker: "TEST".into(),
            yes_bid: Some(45),
            yes_ask: Some(55),
            last_price: None,
            ts: Utc::now(),
        };
        assert_eq!(tick.yes_bid_dollars(), Some(Decimal::new(45, 2)));
        assert_eq!(tick.yes_ask_dollars(), Some(Decimal::new(55, 2)));
    }

    #[test]
    fn test_yes_bid_dollars_none() {
        let tick = PriceTick {
            ticker: "TEST".into(),
            yes_bid: None,
            yes_ask: None,
            last_price: None,
            ts: Utc::now(),
        };
        assert_eq!(tick.yes_bid_dollars(), None);
        assert_eq!(tick.yes_ask_dollars(), None);
    }
}
