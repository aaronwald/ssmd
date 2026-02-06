//! Kraken v2 WebSocket message types
//!
//! Defines the message structures for Kraken's v2 WebSocket API.
//! Uses `#[serde(untagged)]` since Kraken messages don't have a single consistent tag field.

use serde::Deserialize;

/// Incoming WebSocket messages from Kraken v2 API
///
/// Variant order matters for `#[serde(untagged)]` - serde tries each in order.
/// ChannelMessage (has `data`) must come before Heartbeat (no `data`) since both
/// share `channel` and `type` fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum KrakenWsMessage {
    /// Channel data message (ticker, trade, etc.)
    ChannelMessage {
        channel: String,
        #[serde(rename = "type")]
        msg_type: String,
        data: Vec<serde_json::Value>,
    },
    /// Subscription result
    SubscriptionResult {
        method: String,
        success: bool,
        result: Option<serde_json::Value>,
        time_in: Option<String>,
        time_out: Option<String>,
    },
    /// Pong response to app-level ping
    Pong {
        method: String,
        time_in: Option<String>,
        time_out: Option<String>,
    },
    /// Heartbeat (channel=heartbeat, type=update, no data field)
    Heartbeat {
        channel: String,
        #[serde(rename = "type")]
        msg_type: String,
    },
}

/// Kraken ticker data
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenTickerData {
    pub symbol: String,
    pub bid: f64,
    pub bid_qty: f64,
    pub ask: f64,
    pub ask_qty: f64,
    pub last: f64,
    pub volume: f64,
    pub vwap: f64,
    pub high: f64,
    pub low: f64,
    pub change: f64,
    pub change_pct: f64,
}

/// Kraken trade data
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenTradeData {
    pub symbol: String,
    pub side: String,
    pub price: f64,
    pub qty: f64,
    pub ord_type: String,
    pub trade_id: String,
    pub timestamp: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICKER_MESSAGE: &str = r#"{"channel":"ticker","type":"update","data":[{"symbol":"BTC/USD","bid":97000.0,"bid_qty":0.50000000,"ask":97000.1,"ask_qty":1.00000000,"last":97000.0,"volume":1234.56789012,"vwap":96500.0,"low":95000.0,"high":98000.0,"change":500.0,"change_pct":0.52}]}"#;
    const TRADE_MESSAGE: &str = r#"{"channel":"trade","type":"update","data":[{"symbol":"BTC/USD","side":"buy","price":97000.0,"qty":0.001,"ord_type":"market","trade_id":"12345","timestamp":"2026-02-06T12:00:00.000000Z"}]}"#;
    const HEARTBEAT_MESSAGE: &str = r#"{"channel":"heartbeat","type":"update"}"#;
    const PONG_MESSAGE: &str = r#"{"method":"pong","time_in":"2026-02-06T12:00:00.000000Z","time_out":"2026-02-06T12:00:00.000001Z"}"#;
    const SUBSCRIBE_RESULT: &str = r#"{"method":"subscribe","result":{"channel":"ticker","symbol":"BTC/USD"},"success":true,"time_in":"2026-02-06T12:00:00.000000Z","time_out":"2026-02-06T12:00:00.000001Z"}"#;

    #[test]
    fn test_parse_ticker_message() {
        let msg: KrakenWsMessage =
            serde_json::from_str(TICKER_MESSAGE).expect("Failed to parse ticker");

        match msg {
            KrakenWsMessage::ChannelMessage {
                channel,
                msg_type,
                data,
            } => {
                assert_eq!(channel, "ticker");
                assert_eq!(msg_type, "update");
                assert_eq!(data.len(), 1);

                let ticker: KrakenTickerData =
                    serde_json::from_value(data[0].clone()).expect("Failed to parse ticker data");
                assert_eq!(ticker.symbol, "BTC/USD");
                assert_eq!(ticker.bid, 97000.0);
                assert_eq!(ticker.ask, 97000.1);
                assert_eq!(ticker.last, 97000.0);
                assert_eq!(ticker.volume, 1234.56789012);
                assert_eq!(ticker.vwap, 96500.0);
                assert_eq!(ticker.high, 98000.0);
                assert_eq!(ticker.low, 95000.0);
                assert_eq!(ticker.change, 500.0);
                assert_eq!(ticker.change_pct, 0.52);
            }
            _ => panic!("Expected ChannelMessage variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_trade_message() {
        let msg: KrakenWsMessage =
            serde_json::from_str(TRADE_MESSAGE).expect("Failed to parse trade");

        match msg {
            KrakenWsMessage::ChannelMessage {
                channel,
                msg_type,
                data,
            } => {
                assert_eq!(channel, "trade");
                assert_eq!(msg_type, "update");
                assert_eq!(data.len(), 1);

                let trade: KrakenTradeData =
                    serde_json::from_value(data[0].clone()).expect("Failed to parse trade data");
                assert_eq!(trade.symbol, "BTC/USD");
                assert_eq!(trade.side, "buy");
                assert_eq!(trade.price, 97000.0);
                assert_eq!(trade.qty, 0.001);
                assert_eq!(trade.ord_type, "market");
                assert_eq!(trade.trade_id, "12345");
                assert_eq!(trade.timestamp, "2026-02-06T12:00:00.000000Z");
            }
            _ => panic!("Expected ChannelMessage variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_heartbeat_message() {
        let msg: KrakenWsMessage =
            serde_json::from_str(HEARTBEAT_MESSAGE).expect("Failed to parse heartbeat");

        match msg {
            KrakenWsMessage::Heartbeat {
                channel, msg_type, ..
            } => {
                assert_eq!(channel, "heartbeat");
                assert_eq!(msg_type, "update");
            }
            _ => panic!("Expected Heartbeat variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_pong_message() {
        let msg: KrakenWsMessage =
            serde_json::from_str(PONG_MESSAGE).expect("Failed to parse pong");

        match msg {
            KrakenWsMessage::Pong {
                method,
                time_in,
                time_out,
            } => {
                assert_eq!(method, "pong");
                assert!(time_in.is_some());
                assert!(time_out.is_some());
            }
            _ => panic!("Expected Pong variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_subscribe_result() {
        let msg: KrakenWsMessage =
            serde_json::from_str(SUBSCRIBE_RESULT).expect("Failed to parse subscribe result");

        match msg {
            KrakenWsMessage::SubscriptionResult {
                method,
                success,
                result,
                ..
            } => {
                assert_eq!(method, "subscribe");
                assert!(success);
                assert!(result.is_some());
            }
            _ => panic!("Expected SubscriptionResult variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_ticker_data_deserialization() {
        let json = r#"{"symbol":"ETH/USD","bid":3200.5,"bid_qty":10.0,"ask":3201.0,"ask_qty":5.0,"last":3200.75,"volume":50000.0,"vwap":3180.0,"low":3100.0,"high":3300.0,"change":50.0,"change_pct":1.5}"#;
        let data: KrakenTickerData =
            serde_json::from_str(json).expect("Failed to parse ticker data");

        assert_eq!(data.symbol, "ETH/USD");
        assert_eq!(data.bid, 3200.5);
        assert_eq!(data.ask, 3201.0);
        assert_eq!(data.last, 3200.75);
    }

    #[test]
    fn test_trade_data_deserialization() {
        let json = r#"{"symbol":"ETH/USD","side":"sell","price":3200.5,"qty":1.5,"ord_type":"limit","trade_id":"67890","timestamp":"2026-02-06T12:30:00.000000Z"}"#;
        let data: KrakenTradeData =
            serde_json::from_str(json).expect("Failed to parse trade data");

        assert_eq!(data.symbol, "ETH/USD");
        assert_eq!(data.side, "sell");
        assert_eq!(data.price, 3200.5);
        assert_eq!(data.qty, 1.5);
        assert_eq!(data.ord_type, "limit");
    }

    #[test]
    fn test_subscribe_failure_result() {
        let json = r#"{"method":"subscribe","result":null,"success":false,"time_in":"2026-02-06T12:00:00.000000Z","time_out":"2026-02-06T12:00:00.000001Z"}"#;
        let msg: KrakenWsMessage =
            serde_json::from_str(json).expect("Failed to parse failed subscribe");

        match msg {
            KrakenWsMessage::SubscriptionResult {
                success, result, ..
            } => {
                assert!(!success);
                assert!(result.is_none());
            }
            _ => panic!("Expected SubscriptionResult variant"),
        }
    }
}
