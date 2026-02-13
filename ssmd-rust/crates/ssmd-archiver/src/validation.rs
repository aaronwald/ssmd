//! Lightweight field-presence validation for JSONL messages.
//!
//! Checks that critical fields required by downstream parquet generation
//! exist before writing. Missing fields are logged as warnings but data
//! is still written (let DQ catch it downstream).

/// Result of validating a single message.
pub struct ValidationResult {
    pub message_type: Option<String>,
    pub missing_fields: Vec<&'static str>,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.missing_fields.is_empty()
    }
}

/// Per-feed message validator. Checks required field presence using
/// serde_json::Value — no Arrow or schema crate dependency.
pub struct MessageValidator {
    feed: String,
}

impl MessageValidator {
    pub fn new(feed: &str) -> Self {
        Self {
            feed: feed.to_string(),
        }
    }

    /// Validate a parsed JSON message. Returns any missing required fields.
    pub fn validate(&self, json: &serde_json::Value) -> ValidationResult {
        let msg_type = self.detect_type(json);

        let missing_fields = match msg_type.as_deref() {
            Some(t) => self.check_fields(t, json),
            None => vec![], // Unknown/control message type — skip validation
        };

        ValidationResult {
            message_type: msg_type,
            missing_fields,
        }
    }

    /// Detect message type using feed-specific conventions.
    /// Mirrors ssmd-schemas detect_message_type logic.
    fn detect_type(&self, json: &serde_json::Value) -> Option<String> {
        match self.feed.as_str() {
            "kalshi" => json.get("type")?.as_str().map(String::from),
            "kraken-futures" => {
                // Skip subscription/event control messages
                if json.get("event").is_some() {
                    return None;
                }
                json.get("feed")?.as_str().map(String::from)
            }
            "polymarket" => json.get("event_type")?.as_str().map(String::from),
            _ => None,
        }
    }

    fn check_fields(&self, msg_type: &str, json: &serde_json::Value) -> Vec<&'static str> {
        match self.feed.as_str() {
            "kalshi" => check_kalshi(msg_type, json),
            "kraken-futures" => check_kraken_futures(msg_type, json),
            "polymarket" => check_polymarket(msg_type, json),
            _ => vec![],
        }
    }
}

/// Kalshi messages have envelope: {"type":"...", "msg":{...}}
/// Required fields are inside "msg".
fn check_kalshi(msg_type: &str, json: &serde_json::Value) -> Vec<&'static str> {
    // Control messages (subscribed, ok, error) have no data payload
    let msg = match json.get("msg") {
        Some(m) if m.is_object() => m,
        _ => match msg_type {
            "ticker" | "trade" | "market_lifecycle_v2" => return vec!["msg"],
            _ => return vec![],
        },
    };

    match msg_type {
        "ticker" => {
            let mut missing = vec![];
            if msg.get("market_ticker").and_then(|v| v.as_str()).is_none() {
                missing.push("msg.market_ticker");
            }
            if msg.get("ts").and_then(|v| v.as_i64()).is_none() {
                missing.push("msg.ts");
            }
            missing
        }
        "trade" => {
            let mut missing = vec![];
            if msg.get("market_ticker").and_then(|v| v.as_str()).is_none() {
                missing.push("msg.market_ticker");
            }
            if msg.get("trade_id").and_then(|v| v.as_str()).is_none() {
                missing.push("msg.trade_id");
            }
            if msg
                .get("yes_price")
                .or_else(|| msg.get("price"))
                .and_then(|v| v.as_i64())
                .is_none()
            {
                missing.push("msg.yes_price|price");
            }
            if msg.get("count").and_then(|v| v.as_i64()).is_none() {
                missing.push("msg.count");
            }
            if msg
                .get("taker_side")
                .or_else(|| msg.get("side"))
                .and_then(|v| v.as_str())
                .is_none()
            {
                missing.push("msg.taker_side|side");
            }
            if msg.get("ts").and_then(|v| v.as_i64()).is_none() {
                missing.push("msg.ts");
            }
            missing
        }
        "market_lifecycle_v2" => {
            let mut missing = vec![];
            if msg.get("market_ticker").and_then(|v| v.as_str()).is_none() {
                missing.push("msg.market_ticker");
            }
            if msg.get("event_type").and_then(|v| v.as_str()).is_none() {
                missing.push("msg.event_type");
            }
            missing
        }
        _ => vec![], // subscribed, ok, error — no validation
    }
}

/// Kraken Futures V1 messages are flat (no envelope).
fn check_kraken_futures(msg_type: &str, json: &serde_json::Value) -> Vec<&'static str> {
    match msg_type {
        "ticker" => {
            let mut missing = vec![];
            if json.get("product_id").and_then(|v| v.as_str()).is_none() {
                missing.push("product_id");
            }
            if json.get("bid").and_then(|v| v.as_f64()).is_none() {
                missing.push("bid");
            }
            if json.get("ask").and_then(|v| v.as_f64()).is_none() {
                missing.push("ask");
            }
            if json.get("last").and_then(|v| v.as_f64()).is_none() {
                missing.push("last");
            }
            if json.get("volume").and_then(|v| v.as_f64()).is_none() {
                missing.push("volume");
            }
            if json.get("time").and_then(|v| v.as_i64()).is_none() {
                missing.push("time");
            }
            missing
        }
        "trade" => {
            let mut missing = vec![];
            if json.get("product_id").and_then(|v| v.as_str()).is_none() {
                missing.push("product_id");
            }
            if json.get("uid").and_then(|v| v.as_str()).is_none() {
                missing.push("uid");
            }
            if json.get("side").and_then(|v| v.as_str()).is_none() {
                missing.push("side");
            }
            if json.get("price").and_then(|v| v.as_f64()).is_none() {
                missing.push("price");
            }
            if json.get("qty").and_then(|v| v.as_f64()).is_none() {
                missing.push("qty");
            }
            if json.get("time").and_then(|v| v.as_i64()).is_none() {
                missing.push("time");
            }
            missing
        }
        _ => vec![], // ticker_lite, heartbeat, etc.
    }
}

/// Polymarket messages are flat with event_type discriminator.
fn check_polymarket(msg_type: &str, json: &serde_json::Value) -> Vec<&'static str> {
    match msg_type {
        "book" => {
            let mut missing = vec![];
            if json.get("asset_id").and_then(|v| v.as_str()).is_none() {
                missing.push("asset_id");
            }
            if json.get("market").and_then(|v| v.as_str()).is_none() {
                missing.push("market");
            }
            missing
        }
        "last_trade_price" => {
            let mut missing = vec![];
            if json.get("asset_id").and_then(|v| v.as_str()).is_none() {
                missing.push("asset_id");
            }
            if json.get("market").and_then(|v| v.as_str()).is_none() {
                missing.push("market");
            }
            if json.get("price").and_then(|v| v.as_str()).is_none() {
                missing.push("price");
            }
            missing
        }
        "price_change" => {
            let mut missing = vec![];
            if json.get("market").and_then(|v| v.as_str()).is_none() {
                missing.push("market");
            }
            match json.get("price_changes").and_then(|v| v.as_array()) {
                Some(arr) if !arr.is_empty() => {
                    // Validate first item has required fields
                    let first = &arr[0];
                    if first.get("asset_id").and_then(|v| v.as_str()).is_none() {
                        missing.push("price_changes[0].asset_id");
                    }
                    if first.get("price").and_then(|v| v.as_str()).is_none() {
                        missing.push("price_changes[0].price");
                    }
                    if first.get("size").and_then(|v| v.as_str()).is_none() {
                        missing.push("price_changes[0].size");
                    }
                    if first.get("side").and_then(|v| v.as_str()).is_none() {
                        missing.push("price_changes[0].side");
                    }
                }
                Some(_) => missing.push("price_changes (empty)"),
                None => missing.push("price_changes"),
            }
            missing
        }
        "best_bid_ask" => {
            let mut missing = vec![];
            if json.get("market").and_then(|v| v.as_str()).is_none() {
                missing.push("market");
            }
            if json.get("asset_id").and_then(|v| v.as_str()).is_none() {
                missing.push("asset_id");
            }
            missing
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Kalshi
    // -----------------------------------------------------------------------

    #[test]
    fn test_kalshi_ticker_valid() {
        let v = MessageValidator::new("kalshi");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"ticker","sid":1,"msg":{"market_ticker":"KXBTC","yes_bid":50,"ts":1707667200}}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
        assert_eq!(result.message_type.as_deref(), Some("ticker"));
    }

    #[test]
    fn test_kalshi_ticker_missing_ts() {
        let v = MessageValidator::new("kalshi");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"ticker","msg":{"market_ticker":"KXBTC"}}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert_eq!(result.missing_fields, vec!["msg.ts"]);
    }

    #[test]
    fn test_kalshi_ticker_missing_msg() {
        let v = MessageValidator::new("kalshi");
        let json: serde_json::Value =
            serde_json::from_str(r#"{"type":"ticker"}"#).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert_eq!(result.missing_fields, vec!["msg"]);
    }

    #[test]
    fn test_kalshi_trade_valid() {
        let v = MessageValidator::new("kalshi");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"trade","seq":1,"msg":{"market_ticker":"KXBTC","trade_id":"tid-1","yes_price":55,"count":10,"taker_side":"yes","ts":100}}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
    }

    #[test]
    fn test_kalshi_trade_valid_old_field_names() {
        let v = MessageValidator::new("kalshi");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"trade","msg":{"market_ticker":"KXBTC","trade_id":"tid-1","price":55,"count":10,"side":"yes","ts":100}}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
    }

    #[test]
    fn test_kalshi_trade_missing_fields() {
        let v = MessageValidator::new("kalshi");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"trade","msg":{"market_ticker":"KXBTC"}}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert!(result.missing_fields.contains(&"msg.trade_id"));
        assert!(result.missing_fields.contains(&"msg.yes_price|price"));
        assert!(result.missing_fields.contains(&"msg.count"));
        assert!(result.missing_fields.contains(&"msg.taker_side|side"));
        assert!(result.missing_fields.contains(&"msg.ts"));
    }

    #[test]
    fn test_kalshi_lifecycle_valid() {
        let v = MessageValidator::new("kalshi");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"market_lifecycle_v2","msg":{"market_ticker":"KXBTC","event_type":"activated"}}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
    }

    #[test]
    fn test_kalshi_lifecycle_missing_event_type() {
        let v = MessageValidator::new("kalshi");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"market_lifecycle_v2","msg":{"market_ticker":"KXBTC"}}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert_eq!(result.missing_fields, vec!["msg.event_type"]);
    }

    #[test]
    fn test_kalshi_control_messages_skip_validation() {
        let v = MessageValidator::new("kalshi");

        // "subscribed" type — no msg validation needed
        let json: serde_json::Value =
            serde_json::from_str(r#"{"type":"subscribed","msg":{}}"#).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
        assert_eq!(result.message_type.as_deref(), Some("subscribed"));

        // "ok" type
        let json: serde_json::Value =
            serde_json::from_str(r#"{"type":"ok","sid":1}"#).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
    }

    // -----------------------------------------------------------------------
    // Kraken Futures
    // -----------------------------------------------------------------------

    #[test]
    fn test_kraken_futures_ticker_valid() {
        let v = MessageValidator::new("kraken-futures");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"feed":"ticker","product_id":"PF_XBTUSD","bid":65360.0,"ask":65361.0,"last":65367.0,"volume":5826.47,"time":1770920339237}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
        assert_eq!(result.message_type.as_deref(), Some("ticker"));
    }

    #[test]
    fn test_kraken_futures_ticker_missing_product_id() {
        let v = MessageValidator::new("kraken-futures");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"feed":"ticker","bid":1907.3,"ask":1908.0,"last":1907.5,"volume":100.0,"time":1770920339237}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert_eq!(result.missing_fields, vec!["product_id"]);
    }

    #[test]
    fn test_kraken_futures_trade_valid() {
        let v = MessageValidator::new("kraken-futures");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"feed":"trade","product_id":"PF_XBTUSD","uid":"abc-123","side":"sell","type":"fill","seq":100,"qty":0.001,"price":65368.0,"time":1770920339688}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
    }

    #[test]
    fn test_kraken_futures_trade_missing_uid_and_price() {
        let v = MessageValidator::new("kraken-futures");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"feed":"trade","product_id":"PF_XBTUSD","side":"sell","type":"fill","seq":100,"qty":0.001,"time":1770920339688}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert!(result.missing_fields.contains(&"uid"));
        assert!(result.missing_fields.contains(&"price"));
    }

    #[test]
    fn test_kraken_futures_event_skipped() {
        let v = MessageValidator::new("kraken-futures");
        let json: serde_json::Value =
            serde_json::from_str(r#"{"event":"subscribed","feed":"ticker"}"#).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid()); // No type detected, no validation
        assert!(result.message_type.is_none());
    }

    // -----------------------------------------------------------------------
    // Polymarket
    // -----------------------------------------------------------------------

    #[test]
    fn test_polymarket_book_valid() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"book","asset_id":"123","market":"0xabc","buys":[],"sells":[]}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
        assert_eq!(result.message_type.as_deref(), Some("book"));
    }

    #[test]
    fn test_polymarket_book_missing_market() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"book","asset_id":"123"}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert_eq!(result.missing_fields, vec!["market"]);
    }

    #[test]
    fn test_polymarket_trade_valid() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"last_trade_price","asset_id":"123","market":"0xabc","price":"0.55"}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
    }

    #[test]
    fn test_polymarket_trade_missing_price() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"last_trade_price","asset_id":"123","market":"0xabc"}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert_eq!(result.missing_fields, vec!["price"]);
    }

    #[test]
    fn test_polymarket_price_change_valid() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"price_change","market":"0xabc","price_changes":[{"asset_id":"123","price":"0.55","size":"100","side":"BUY"}]}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
        assert_eq!(result.message_type.as_deref(), Some("price_change"));
    }

    #[test]
    fn test_polymarket_price_change_missing_market() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"price_change","price_changes":[{"asset_id":"123","price":"0.55","size":"100","side":"BUY"}]}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert_eq!(result.missing_fields, vec!["market"]);
    }

    #[test]
    fn test_polymarket_price_change_missing_price_changes() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"price_change","market":"0xabc"}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert!(result.missing_fields.contains(&"price_changes"));
    }

    #[test]
    fn test_polymarket_price_change_empty_array() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"price_change","market":"0xabc","price_changes":[]}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert!(result.missing_fields.contains(&"price_changes (empty)"));
    }

    #[test]
    fn test_polymarket_price_change_item_missing_fields() {
        let v = MessageValidator::new("polymarket");
        // First item missing price and side
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"price_change","market":"0xabc","price_changes":[{"asset_id":"123","size":"100"}]}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert!(result.missing_fields.contains(&"price_changes[0].price"));
        assert!(result.missing_fields.contains(&"price_changes[0].side"));
    }

    #[test]
    fn test_polymarket_best_bid_ask_valid() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"best_bid_ask","market":"0xabc","asset_id":"123","best_bid":"0.55","best_ask":"0.56"}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
        assert_eq!(result.message_type.as_deref(), Some("best_bid_ask"));
    }

    #[test]
    fn test_polymarket_best_bid_ask_missing_asset_id() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"best_bid_ask","market":"0xabc"}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert_eq!(result.missing_fields, vec!["asset_id"]);
    }

    #[test]
    fn test_polymarket_best_bid_ask_missing_market() {
        let v = MessageValidator::new("polymarket");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"best_bid_ask","asset_id":"123"}"#,
        ).unwrap();
        let result = v.validate(&json);
        assert!(!result.is_valid());
        assert_eq!(result.missing_fields, vec!["market"]);
    }

    // -----------------------------------------------------------------------
    // Unknown feed
    // -----------------------------------------------------------------------

    #[test]
    fn test_unknown_feed_skips_validation() {
        let v = MessageValidator::new("unknown-feed");
        let json: serde_json::Value =
            serde_json::from_str(r#"{"anything":"goes"}"#).unwrap();
        let result = v.validate(&json);
        assert!(result.is_valid());
        assert!(result.message_type.is_none());
    }
}
