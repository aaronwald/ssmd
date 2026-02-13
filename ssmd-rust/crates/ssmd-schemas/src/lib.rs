use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use arrow::datatypes::Schema;
use arrow::error::ArrowError;
use arrow::record_batch::RecordBatch;

pub mod kalshi;
pub mod kraken;
pub mod kraken_futures;
pub mod polymarket;

/// Trait for converting raw JSON messages to Arrow RecordBatches.
pub trait MessageSchema: Send + Sync {
    /// Schema name for versioning (e.g., "kalshi_trade").
    fn schema_name(&self) -> &str;

    /// Semver version of this schema (e.g., "1.1.0").
    fn schema_version(&self) -> &str;

    /// Arrow schema for this message type.
    fn schema(&self) -> Arc<Schema>;

    /// Message type name (e.g., "ticker", "trade").
    fn message_type(&self) -> &str;

    /// Parse a batch of JSON messages into a RecordBatch.
    /// Each entry is (raw_json_bytes, nats_seq, received_at_micros).
    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError>;

    /// Extract dedup key (hash of primary key fields) from JSON.
    /// Returns None if message type doesn't match this schema.
    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64>;
}

/// Registry mapping (feed, detected_type) to the right schema.
pub struct SchemaRegistry {
    feed: String,
    schemas: HashMap<String, Box<dyn MessageSchema>>,
}

impl SchemaRegistry {
    pub fn for_feed(feed: &str) -> Self {
        let mut schemas: HashMap<String, Box<dyn MessageSchema>> = HashMap::new();

        match feed {
            "kalshi" => {
                schemas.insert(
                    "ticker".to_string(),
                    Box::new(kalshi::KalshiTickerSchema),
                );
                schemas.insert("trade".to_string(), Box::new(kalshi::KalshiTradeSchema));
                schemas.insert(
                    "market_lifecycle_v2".to_string(),
                    Box::new(kalshi::KalshiLifecycleSchema),
                );
            }
            "kraken" => {
                schemas.insert(
                    "ticker".to_string(),
                    Box::new(kraken::KrakenTickerSchema),
                );
                schemas.insert("trade".to_string(), Box::new(kraken::KrakenTradeSchema));
            }
            "kraken-futures" => {
                schemas.insert(
                    "ticker".to_string(),
                    Box::new(kraken_futures::KrakenFuturesTickerSchema),
                );
                schemas.insert(
                    "trade".to_string(),
                    Box::new(kraken_futures::KrakenFuturesTradeSchema),
                );
            }
            "polymarket" => {
                schemas.insert(
                    "book".to_string(),
                    Box::new(polymarket::PolymarketBookSchema),
                );
                schemas.insert(
                    "last_trade_price".to_string(),
                    Box::new(polymarket::PolymarketTradeSchema),
                );
                schemas.insert(
                    "price_change".to_string(),
                    Box::new(polymarket::PolymarketPriceChangeSchema),
                );
                schemas.insert(
                    "best_bid_ask".to_string(),
                    Box::new(polymarket::PolymarketBestBidAskSchema),
                );
            }
            _ => {}
        }

        SchemaRegistry {
            feed: feed.to_string(),
            schemas,
        }
    }

    pub fn get(&self, message_type: &str) -> Option<&dyn MessageSchema> {
        self.schemas.get(message_type).map(|s| s.as_ref())
    }

    pub fn detect_and_get(
        &self,
        json: &serde_json::Value,
    ) -> Option<(&str, &dyn MessageSchema)> {
        let msg_type = detect_message_type(&self.feed, json)?;
        let schema = self.schemas.get(&msg_type)?;
        Some((schema.message_type(), schema.as_ref()))
    }
}

/// Detect message type from raw JSON based on feed-specific conventions.
pub fn detect_message_type(feed: &str, json: &serde_json::Value) -> Option<String> {
    match feed {
        "kalshi" => {
            // Kalshi uses "type" field: "ticker", "trade", "market_lifecycle_v2", etc.
            json.get("type")?.as_str().map(String::from)
        }
        "kraken" => {
            // Kraken Spot V2 uses "channel" field: "ticker", "trade", "heartbeat"
            // Messages without "data" are control messages (skip)
            json.get("data")?;
            json.get("channel")?.as_str().map(String::from)
        }
        "kraken-futures" => {
            // Kraken Futures V1 uses flat "feed" field: "ticker", "trade", "ticker_lite", etc.
            // Skip snapshot/subscription messages (have "event" field)
            if json.get("event").is_some() {
                return None;
            }
            json.get("feed")?.as_str().map(String::from)
        }
        "polymarket" => {
            // Polymarket uses "event_type" field
            json.get("event_type")?.as_str().map(String::from)
        }
        _ => None,
    }
}

/// Helper to compute a dedup hash from string-like components.
pub fn hash_dedup_key(parts: &[&str]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_kalshi() {
        let reg = SchemaRegistry::for_feed("kalshi");
        assert!(reg.get("ticker").is_some());
        assert!(reg.get("trade").is_some());
        assert!(reg.get("market_lifecycle_v2").is_some());
        assert!(reg.get("unknown").is_none());
    }

    #[test]
    fn test_registry_kraken() {
        let reg = SchemaRegistry::for_feed("kraken");
        assert!(reg.get("ticker").is_some());
        assert!(reg.get("trade").is_some());
        assert!(reg.get("heartbeat").is_none());
    }

    #[test]
    fn test_registry_polymarket() {
        let reg = SchemaRegistry::for_feed("polymarket");
        assert!(reg.get("book").is_some());
        assert!(reg.get("last_trade_price").is_some());
        assert!(reg.get("price_change").is_some());
        assert!(reg.get("best_bid_ask").is_some());
        assert!(reg.get("new_market").is_none());
    }

    #[test]
    fn test_registry_kraken_futures() {
        let reg = SchemaRegistry::for_feed("kraken-futures");
        assert!(reg.get("ticker").is_some());
        assert!(reg.get("trade").is_some());
        assert!(reg.get("heartbeat").is_none());
    }

    #[test]
    fn test_detect_kraken_futures_ticker() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"feed":"ticker","product_id":"PF_XBTUSD","bid":65360.0}"#)
                .unwrap();
        assert_eq!(
            detect_message_type("kraken-futures", &json),
            Some("ticker".into())
        );
    }

    #[test]
    fn test_detect_kraken_futures_trade() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"feed":"trade","product_id":"PF_XBTUSD","uid":"abc"}"#)
                .unwrap();
        assert_eq!(
            detect_message_type("kraken-futures", &json),
            Some("trade".into())
        );
    }

    #[test]
    fn test_detect_kraken_futures_event_skipped() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"event":"subscribed","feed":"ticker"}"#).unwrap();
        assert_eq!(detect_message_type("kraken-futures", &json), None);
    }

    #[test]
    fn test_registry_unknown_feed() {
        let reg = SchemaRegistry::for_feed("unknown");
        assert!(reg.get("ticker").is_none());
    }

    #[test]
    fn test_detect_kalshi_ticker() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"type":"ticker","msg":{}}"#).unwrap();
        assert_eq!(detect_message_type("kalshi", &json), Some("ticker".into()));
    }

    #[test]
    fn test_detect_kalshi_subscribed_skipped() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"type":"subscribed","msg":{}}"#).unwrap();
        let detected = detect_message_type("kalshi", &json);
        assert_eq!(detected, Some("subscribed".into()));
        let reg = SchemaRegistry::for_feed("kalshi");
        assert!(reg.get("subscribed").is_none());
    }

    #[test]
    fn test_detect_kraken_ticker() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"channel":"ticker","type":"update","data":[{}]}"#).unwrap();
        assert_eq!(detect_message_type("kraken", &json), Some("ticker".into()));
    }

    #[test]
    fn test_detect_kraken_heartbeat_skipped() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"channel":"heartbeat","type":"update"}"#).unwrap();
        assert_eq!(detect_message_type("kraken", &json), None);
    }

    #[test]
    fn test_detect_kraken_subscription_result_skipped() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"method":"subscribe","success":true,"result":{"channel":"ticker"}}"#,
        )
        .unwrap();
        assert_eq!(detect_message_type("kraken", &json), None);
    }

    #[test]
    fn test_detect_polymarket_book() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"event_type":"book","asset_id":"123"}"#).unwrap();
        assert_eq!(
            detect_message_type("polymarket", &json),
            Some("book".into())
        );
    }

    #[test]
    fn test_detect_polymarket_trade() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"event_type":"last_trade_price","asset_id":"123"}"#).unwrap();
        assert_eq!(
            detect_message_type("polymarket", &json),
            Some("last_trade_price".into())
        );
    }

    #[test]
    fn test_detect_polymarket_price_change() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"event_type":"price_change","market":"0xabc","price_changes":[]}"#).unwrap();
        assert_eq!(
            detect_message_type("polymarket", &json),
            Some("price_change".into())
        );
    }

    #[test]
    fn test_detect_polymarket_best_bid_ask() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"event_type":"best_bid_ask","market":"0xabc","asset_id":"123"}"#).unwrap();
        assert_eq!(
            detect_message_type("polymarket", &json),
            Some("best_bid_ask".into())
        );
    }

    #[test]
    fn test_polymarket_schema_names() {
        let reg = SchemaRegistry::for_feed("polymarket");

        let price_change = reg.get("price_change").unwrap();
        assert_eq!(price_change.schema_name(), "polymarket_price_change");
        assert_eq!(price_change.schema_version(), "1.0.0");

        let best_bid_ask = reg.get("best_bid_ask").unwrap();
        assert_eq!(best_bid_ask.schema_name(), "polymarket_best_bid_ask");
        assert_eq!(best_bid_ask.schema_version(), "1.0.0");
    }

    #[test]
    fn test_detect_and_get() {
        let reg = SchemaRegistry::for_feed("kalshi");
        let json: serde_json::Value =
            serde_json::from_str(r#"{"type":"ticker","msg":{}}"#).unwrap();
        let (msg_type, schema) = reg.detect_and_get(&json).unwrap();
        assert_eq!(msg_type, "ticker");
        assert_eq!(schema.schema().fields().len(), 12);
    }

    #[test]
    fn test_hash_dedup_key_stable() {
        let h1 = hash_dedup_key(&["ticker", "KXBTC", "1707667200"]);
        let h2 = hash_dedup_key(&["ticker", "KXBTC", "1707667200"]);
        assert_eq!(h1, h2);

        let h3 = hash_dedup_key(&["ticker", "KXBTC", "1707667201"]);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_schema_name_and_version() {
        let reg = SchemaRegistry::for_feed("kalshi");
        let ticker = reg.get("ticker").unwrap();
        assert_eq!(ticker.schema_name(), "kalshi_ticker");
        assert_eq!(ticker.schema_version(), "1.1.0");

        let trade = reg.get("trade").unwrap();
        assert_eq!(trade.schema_name(), "kalshi_trade");
        assert_eq!(trade.schema_version(), "1.1.0");

        let lifecycle = reg.get("market_lifecycle_v2").unwrap();
        assert_eq!(lifecycle.schema_name(), "kalshi_lifecycle");
        assert_eq!(lifecycle.schema_version(), "1.0.0");
    }
}
