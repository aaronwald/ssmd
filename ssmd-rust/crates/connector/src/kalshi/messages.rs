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

/// Incoming WebSocket messages from Kalshi
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum WsMessage {
    Subscribed { id: u64 },
    Unsubscribed { id: u64 },
    Ticker { msg: TickerData },
    Trade { msg: TradeData },
    OrderbookSnapshot { msg: OrderbookData },
    OrderbookDelta { msg: OrderbookData },
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

    /// Subscribed confirmation message
    const SUBSCRIBED_MESSAGE: &str = r#"{"type":"subscribed","id":1}"#;

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
            WsMessage::Subscribed { id } => {
                assert_eq!(id, 1);
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
            },
        };

        let json = serde_json::to_string(&cmd).expect("Failed to serialize");
        assert!(json.contains(r#""market_ticker":"KXTEST-123""#));
    }
}
