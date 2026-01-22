//! Kalshi WebSocket message types
//!
//! Defines the message structures for Kalshi's WebSocket API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

/// Custom deserializer for Kalshi timestamps (Unix timestamp in seconds as integer)
pub fn deserialize_unix_timestamp<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let ts = i64::deserialize(deserializer)?;
    DateTime::from_timestamp(ts, 0)
        .ok_or_else(|| D::Error::custom(format!("Invalid unix timestamp: {}", ts)))
}

/// Custom deserializer for optional Kalshi timestamps (Unix timestamp in seconds as integer)
pub fn deserialize_optional_unix_timestamp<'de, D>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let opt_ts: Option<i64> = Option::deserialize(deserializer)?;
    match opt_ts {
        Some(ts) => DateTime::from_timestamp(ts, 0)
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("Invalid unix timestamp: {}", ts))),
        None => Ok(None),
    }
}

/// Subscription confirmation data (inside "msg" field)
#[derive(Debug, Clone, Deserialize)]
pub struct SubscribedData {
    pub channel: String,
    pub sid: u64,
}

/// Error data (inside "msg" field)
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorData {
    pub code: i64,
    pub msg: String,
}

/// Incoming WebSocket messages from Kalshi
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum WsMessage {
    /// Subscription confirmed (for some channels like orderbook_delta)
    Subscribed {
        id: u64,
        /// Subscription details including channel and sid
        #[serde(default)]
        msg: Option<SubscribedData>,
    },
    /// Subscription confirmed (for ticker/trade channels)
    Ok {
        id: u64,
        /// Subscription ID at top level
        #[serde(default)]
        sid: Option<u64>,
        /// Sequence number
        #[serde(default)]
        seq: Option<u64>,
        /// List of subscribed market tickers
        #[serde(default)]
        market_tickers: Option<Vec<String>>,
    },
    Unsubscribed { id: u64 },
    Ticker { msg: TickerData },
    Trade { msg: TradeData },
    OrderbookSnapshot { msg: OrderbookData },
    OrderbookDelta { msg: OrderbookData },
    /// Market lifecycle events (market_lifecycle_v2 channel)
    MarketLifecycleV2 {
        #[serde(default)]
        sid: Option<u64>,
        msg: MarketLifecycleData,
    },
    /// Event lifecycle events (parent event creation)
    EventLifecycle {
        #[serde(default)]
        sid: Option<u64>,
        msg: EventLifecycleData,
    },
    Error {
        id: Option<u64>,
        /// Error details with code and message
        #[serde(default)]
        msg: Option<ErrorData>,
    },
    #[serde(other)]
    Unknown,
}

/// Ticker update data
#[derive(Debug, Clone, Deserialize)]
pub struct TickerData {
    pub market_ticker: String,
    pub yes_bid: Option<i64>,
    pub yes_ask: Option<i64>,
    pub no_bid: Option<i64>,
    pub no_ask: Option<i64>,
    #[serde(alias = "price")]
    pub last_price: Option<i64>,
    pub volume: Option<i64>,
    pub open_interest: Option<i64>,
    #[serde(deserialize_with = "deserialize_unix_timestamp")]
    pub ts: DateTime<Utc>,
}

/// Trade execution data
#[derive(Debug, Clone, Deserialize)]
pub struct TradeData {
    pub market_ticker: String,
    #[serde(alias = "yes_price")]
    pub price: i64,
    pub count: i64,
    #[serde(alias = "taker_side")]
    pub side: String,
    #[serde(deserialize_with = "deserialize_unix_timestamp")]
    pub ts: DateTime<Utc>,
}

/// Orderbook data (snapshot or delta)
#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookData {
    pub market_ticker: String,
    pub yes: Option<Vec<(i64, i64)>>, // (price, quantity)
    pub no: Option<Vec<(i64, i64)>>,
}

/// Market lifecycle event data (market_lifecycle_v2 channel)
///
/// Event types: created, activated, deactivated, close_date_updated, determined, settled
#[derive(Debug, Clone, Deserialize)]
pub struct MarketLifecycleData {
    pub market_ticker: String,
    pub event_type: String,
    #[serde(default, deserialize_with = "deserialize_optional_unix_timestamp")]
    pub open_ts: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "deserialize_optional_unix_timestamp")]
    pub close_ts: Option<DateTime<Utc>>,
    #[serde(default)]
    pub additional_metadata: Option<serde_json::Value>,
}

/// Event lifecycle data (event creation events)
#[derive(Debug, Clone, Deserialize)]
pub struct EventLifecycleData {
    pub event_ticker: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub sub_title: Option<String>,
    #[serde(default)]
    pub collateral_return_type: Option<String>,
    #[serde(default)]
    pub series_ticker: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_unix_timestamp")]
    pub strike_date: Option<DateTime<Utc>>,
}

/// WebSocket command message (outgoing)
#[derive(Debug, Serialize)]
pub struct WsCommand {
    pub id: u64,
    pub cmd: String,
    pub params: WsParams,
}

/// WebSocket command parameters
#[derive(Debug, Serialize)]
pub struct WsParams {
    pub channels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_tickers: Option<Vec<String>>,
    /// Subscription IDs to update (for update_subscription command)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sids: Option<Vec<u64>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real ticker message captured from Kalshi production API
    const TICKER_MESSAGE: &str = r#"{"type":"ticker","sid":1,"msg":{"market_id":"2ee24704-7e13-4248-97f5-3b7ffabf4325","market_ticker":"KXRANKLISTGOOGLESEARCH-26JAN-BIA","price":12,"yes_bid":11,"yes_ask":12,"price_dollars":"0.1200","yes_bid_dollars":"0.1100","yes_ask_dollars":"0.1200","volume":351970,"open_interest":182646,"dollar_volume":175985,"dollar_open_interest":91323,"ts":1732579880,"Clock":6598272994}}"#;

    /// Ticker with zero/empty price fields
    const TICKER_ZERO_PRICE: &str = r#"{"type":"ticker","sid":1,"msg":{"market_id":"5f2c3445-bdee-42d8-a8ae-648dd1fedb22","market_ticker":"KXBRASILEIROGAME-25NOV29CEACRU-CRU","price":0,"yes_bid":37,"yes_ask":43,"price_dollars":"","yes_bid_dollars":"0.3700","yes_ask_dollars":"0.4300","volume":0,"open_interest":0,"dollar_volume":0,"dollar_open_interest":0,"ts":1732579880,"Clock":6598272643}}"#;

    /// Trade message
    const TRADE_MESSAGE: &str = r#"{"type":"trade","sid":1,"msg":{"market_ticker":"KXTEST-123","price":50,"count":10,"side":"yes","ts":1732579880}}"#;

    /// Subscribed confirmation message (without msg - older format)
    const SUBSCRIBED_MESSAGE: &str = r#"{"type":"subscribed","id":1}"#;

    /// Subscribed confirmation message with sid (current format)
    const SUBSCRIBED_MESSAGE_WITH_SID: &str = r#"{"type":"subscribed","id":1,"msg":{"channel":"ticker","sid":42}}"#;

    /// Ok confirmation message (for ticker/trade subscriptions)
    const OK_MESSAGE: &str = r#"{"id":123,"sid":456,"seq":222,"type":"ok","market_tickers":["MARKET-1","MARKET-2","MARKET-3"]}"#;

    /// Error message
    const ERROR_MESSAGE: &str = r#"{"id":123,"type":"error","msg":{"code":6,"msg":"Already subscribed"}}"#;

    #[test]
    fn test_parse_ticker_message() {
        let msg: WsMessage = serde_json::from_str(TICKER_MESSAGE).expect("Failed to parse ticker");

        match msg {
            WsMessage::Ticker { msg } => {
                assert_eq!(msg.market_ticker, "KXRANKLISTGOOGLESEARCH-26JAN-BIA");
                assert_eq!(msg.yes_bid, Some(11));
                assert_eq!(msg.yes_ask, Some(12));
                assert_eq!(msg.last_price, Some(12));
                assert_eq!(msg.volume, Some(351970));
                assert_eq!(msg.open_interest, Some(182646));
                assert_eq!(msg.ts.timestamp(), 1732579880);
            }
            _ => panic!("Expected Ticker variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_ticker_with_zero_price() {
        let msg: WsMessage =
            serde_json::from_str(TICKER_ZERO_PRICE).expect("Failed to parse ticker");

        match msg {
            WsMessage::Ticker { msg } => {
                assert_eq!(msg.market_ticker, "KXBRASILEIROGAME-25NOV29CEACRU-CRU");
                assert_eq!(msg.last_price, Some(0));
                assert_eq!(msg.volume, Some(0));
            }
            _ => panic!("Expected Ticker variant"),
        }
    }

    #[test]
    fn test_parse_trade_message() {
        let msg: WsMessage = serde_json::from_str(TRADE_MESSAGE).expect("Failed to parse trade");

        match msg {
            WsMessage::Trade { msg } => {
                assert_eq!(msg.market_ticker, "KXTEST-123");
                assert_eq!(msg.price, 50);
                assert_eq!(msg.count, 10);
                assert_eq!(msg.side, "yes");
                assert_eq!(msg.ts.timestamp(), 1732579880);
            }
            _ => panic!("Expected Trade variant"),
        }
    }

    #[test]
    fn test_parse_subscribed_message() {
        let msg: WsMessage =
            serde_json::from_str(SUBSCRIBED_MESSAGE).expect("Failed to parse subscribed");

        match msg {
            WsMessage::Subscribed { id, msg } => {
                assert_eq!(id, 1);
                assert!(msg.is_none()); // Basic subscribed message has no msg
            }
            _ => panic!("Expected Subscribed variant"),
        }
    }

    #[test]
    fn test_parse_subscribed_message_with_sid() {
        let msg: WsMessage =
            serde_json::from_str(SUBSCRIBED_MESSAGE_WITH_SID).expect("Failed to parse subscribed");

        match msg {
            WsMessage::Subscribed { id, msg } => {
                assert_eq!(id, 1);
                let data = msg.expect("Expected msg field");
                assert_eq!(data.channel, "ticker");
                assert_eq!(data.sid, 42);
            }
            _ => panic!("Expected Subscribed variant"),
        }
    }

    #[test]
    fn test_unknown_message_type() {
        let unknown = r#"{"type":"some_future_type","data":"test"}"#;
        let msg: WsMessage = serde_json::from_str(unknown).expect("Failed to parse unknown");

        assert!(matches!(msg, WsMessage::Unknown));
    }

    #[test]
    fn test_unix_timestamp_deserializer() {
        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "deserialize_unix_timestamp")]
            ts: DateTime<Utc>,
        }

        let json = r#"{"ts":1732579880}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to parse timestamp");
        assert_eq!(result.ts.timestamp(), 1732579880);
    }

    #[test]
    fn test_ticker_data_optional_fields() {
        let minimal = r#"{"market_ticker":"TEST","ts":1732579880}"#;
        let data: TickerData = serde_json::from_str(minimal).expect("Failed to parse minimal");

        assert_eq!(data.market_ticker, "TEST");
        assert!(data.yes_bid.is_none());
        assert!(data.yes_ask.is_none());
        assert!(data.last_price.is_none());
        assert!(data.volume.is_none());
    }

    #[test]
    fn test_ws_command_serialization() {
        let cmd = WsCommand {
            id: 1,
            cmd: "subscribe".to_string(),
            params: WsParams {
                channels: vec!["ticker".to_string()],
                market_ticker: None,
                market_tickers: None,
                sids: None,
            },
        };

        let json = serde_json::to_string(&cmd).expect("Failed to serialize");
        assert!(json.contains(r#""id":1"#));
        assert!(json.contains(r#""cmd":"subscribe""#));
        assert!(json.contains(r#""channels":["ticker"]"#));
        // market_ticker should be omitted when None
        assert!(!json.contains("market_ticker"));
    }

    #[test]
    fn test_ws_command_with_market_ticker() {
        let cmd = WsCommand {
            id: 2,
            cmd: "subscribe".to_string(),
            params: WsParams {
                channels: vec!["trade".to_string()],
                market_ticker: Some("KXTEST-123".to_string()),
                market_tickers: None,
                sids: None,
            },
        };

        let json = serde_json::to_string(&cmd).expect("Failed to serialize");
        assert!(json.contains(r#""market_ticker":"KXTEST-123""#));
    }

    #[test]
    fn test_ws_command_with_market_tickers_array() {
        let cmd = WsCommand {
            id: 3,
            cmd: "subscribe".to_string(),
            params: WsParams {
                channels: vec!["ticker".to_string()],
                market_ticker: None,
                market_tickers: Some(vec!["KXTEST-1".to_string(), "KXTEST-2".to_string()]),
                sids: None,
            },
        };

        let json = serde_json::to_string(&cmd).expect("Failed to serialize");
        assert!(json.contains(r#""market_tickers":["KXTEST-1","KXTEST-2"]"#));
        // market_ticker should be omitted when None
        assert!(!json.contains(r#""market_ticker""#));
    }

    #[test]
    fn test_parse_ok_message() {
        let msg: WsMessage = serde_json::from_str(OK_MESSAGE).expect("Failed to parse ok");

        match msg {
            WsMessage::Ok {
                id,
                sid,
                seq,
                market_tickers,
            } => {
                assert_eq!(id, 123);
                assert_eq!(sid, Some(456));
                assert_eq!(seq, Some(222));
                let tickers = market_tickers.expect("Expected market_tickers");
                assert_eq!(tickers.len(), 3);
                assert_eq!(tickers[0], "MARKET-1");
                assert_eq!(tickers[1], "MARKET-2");
                assert_eq!(tickers[2], "MARKET-3");
            }
            _ => panic!("Expected Ok variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_error_message() {
        let msg: WsMessage = serde_json::from_str(ERROR_MESSAGE).expect("Failed to parse error");

        match msg {
            WsMessage::Error { id, msg } => {
                assert_eq!(id, Some(123));
                let error_data = msg.expect("Expected error msg");
                assert_eq!(error_data.code, 6);
                assert_eq!(error_data.msg, "Already subscribed");
            }
            _ => panic!("Expected Error variant, got {:?}", msg),
        }
    }

    /// Market lifecycle message (market_lifecycle_v2 channel)
    const LIFECYCLE_MESSAGE: &str = r#"{"type":"market_lifecycle_v2","sid":13,"msg":{"market_ticker":"KXBTCD-26JAN2310-T105000","event_type":"activated","open_ts":1737554400,"close_ts":1737558000,"additional_metadata":{"settlement_value":null}}}"#;

    /// Market lifecycle message with minimal fields
    const LIFECYCLE_MESSAGE_MINIMAL: &str = r#"{"type":"market_lifecycle_v2","sid":13,"msg":{"market_ticker":"KXBTCD-26JAN2310-T105000","event_type":"created"}}"#;

    /// Event lifecycle message
    const EVENT_LIFECYCLE_MESSAGE: &str = r#"{"type":"event_lifecycle","sid":14,"msg":{"event_ticker":"KXBTCD-26JAN2310","title":"Bitcoin Price","sub_title":"Will BTC exceed $105,000?","series_ticker":"KXBTCD"}}"#;

    #[test]
    fn test_parse_market_lifecycle_message() {
        let msg: WsMessage =
            serde_json::from_str(LIFECYCLE_MESSAGE).expect("Failed to parse lifecycle");

        match msg {
            WsMessage::MarketLifecycleV2 { sid, msg } => {
                assert_eq!(sid, Some(13));
                assert_eq!(msg.market_ticker, "KXBTCD-26JAN2310-T105000");
                assert_eq!(msg.event_type, "activated");
                assert!(msg.open_ts.is_some());
                assert_eq!(msg.open_ts.unwrap().timestamp(), 1737554400);
                assert!(msg.close_ts.is_some());
                assert_eq!(msg.close_ts.unwrap().timestamp(), 1737558000);
                assert!(msg.additional_metadata.is_some());
            }
            _ => panic!("Expected MarketLifecycleV2 variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_market_lifecycle_message_minimal() {
        let msg: WsMessage =
            serde_json::from_str(LIFECYCLE_MESSAGE_MINIMAL).expect("Failed to parse lifecycle");

        match msg {
            WsMessage::MarketLifecycleV2 { sid, msg } => {
                assert_eq!(sid, Some(13));
                assert_eq!(msg.market_ticker, "KXBTCD-26JAN2310-T105000");
                assert_eq!(msg.event_type, "created");
                assert!(msg.open_ts.is_none());
                assert!(msg.close_ts.is_none());
                assert!(msg.additional_metadata.is_none());
            }
            _ => panic!("Expected MarketLifecycleV2 variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_event_lifecycle_message() {
        let msg: WsMessage =
            serde_json::from_str(EVENT_LIFECYCLE_MESSAGE).expect("Failed to parse event lifecycle");

        match msg {
            WsMessage::EventLifecycle { sid, msg } => {
                assert_eq!(sid, Some(14));
                assert_eq!(msg.event_ticker, "KXBTCD-26JAN2310");
                assert_eq!(msg.title, Some("Bitcoin Price".to_string()));
                assert_eq!(msg.sub_title, Some("Will BTC exceed $105,000?".to_string()));
                assert_eq!(msg.series_ticker, Some("KXBTCD".to_string()));
            }
            _ => panic!("Expected EventLifecycle variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_optional_timestamp_deserializer() {
        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(default, deserialize_with = "deserialize_optional_unix_timestamp")]
            ts: Option<DateTime<Utc>>,
        }

        // Test with timestamp
        let json = r#"{"ts":1732579880}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to parse");
        assert!(result.ts.is_some());
        assert_eq!(result.ts.unwrap().timestamp(), 1732579880);

        // Test with null
        let json_null = r#"{"ts":null}"#;
        let result_null: TestStruct = serde_json::from_str(json_null).expect("Failed to parse");
        assert!(result_null.ts.is_none());

        // Test with missing field
        let json_missing = r#"{}"#;
        let result_missing: TestStruct = serde_json::from_str(json_missing).expect("Failed to parse");
        assert!(result_missing.ts.is_none());
    }
}
