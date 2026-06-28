//! Binance combined-stream WebSocket message types
//!
//! Defines the message structures for Binance's public spot WebSocket API as
//! delivered over the **combined stream** endpoint
//! (`/stream?streams=btcusdt@trade/...`). Each inbound frame is wrapped:
//!
//! ```json
//! {"stream":"btcusdt@trade","data":{"e":"trade","E":123,"s":"BTCUSDT","t":1,
//!  "p":"0.001","q":"100","T":123,"m":true,"M":true}}
//! ```
//!
//! Uses `#[serde(untagged)]` because Binance frames have no single consistent
//! tag field — a data frame carries `stream`+`data`, a command response carries
//! `result`+`id`, and an error carries `error`.

use serde::Deserialize;

/// Incoming WebSocket messages from the Binance combined-stream API.
///
/// Variant order matters for `#[serde(untagged)]` — serde tries each in order.
/// `Combined` (has `stream`+`data`) must come first; it is the only variant
/// that yields tradeable data. Command/error frames only ever appear if a
/// SUBSCRIBE control message is sent (we subscribe via the URL query, so they
/// are rare) but are modelled so the receiver never crashes on one.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum BinanceWsMessage {
    /// Combined-stream data frame: `{"stream":"btcusdt@trade","data":{...}}`.
    Combined {
        stream: String,
        data: BinanceStreamData,
    },
    /// Response to a SUBSCRIBE/UNSUBSCRIBE/LIST command: `{"result":...,"id":N}`.
    CommandResult {
        result: Option<serde_json::Value>,
        id: serde_json::Value,
    },
    /// Error frame: `{"error":{"code":...,"msg":"..."}}`.
    Error { error: serde_json::Value },
}

/// Minimal view of the inner `data` object shared by every Binance stream
/// event. Only the discriminator (`e`) and symbol (`s`) are needed to decide
/// whether to forward a frame and where to route it. Full trade fields are
/// modelled separately by [`BinanceTradeData`] for callers that need them.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceStreamData {
    /// Event type discriminator, e.g. `"trade"`.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Symbol, e.g. `"BTCUSDT"`.
    #[serde(rename = "s")]
    pub symbol: String,
}

/// Full Binance `@trade` payload (the inner `data` object of a trade frame).
///
/// Note: `price` and `quantity` arrive as **strings** on the Binance wire
/// (`"p":"0.001"`). They are kept as `String` here — the connector publishes
/// raw frames verbatim; numeric normalization happens downstream
/// (`ssmd-bar-cache` for 1m bars, `ssmd-schemas` for parquet columns).
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceTradeData {
    /// Event type, always `"trade"`.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time (epoch milliseconds).
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol, e.g. `"BTCUSDT"`.
    #[serde(rename = "s")]
    pub symbol: String,
    /// Trade id (per-symbol monotonic integer).
    #[serde(rename = "t")]
    pub trade_id: i64,
    /// Price (string on the wire).
    #[serde(rename = "p")]
    pub price: String,
    /// Quantity (string on the wire).
    #[serde(rename = "q")]
    pub quantity: String,
    /// Trade time (epoch milliseconds) — the authoritative trade timestamp.
    #[serde(rename = "T")]
    pub trade_time: i64,
    /// Whether the buyer is the market maker.
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRADE_FRAME: &str = r#"{"stream":"btcusdt@trade","data":{"e":"trade","E":1719580800000,"s":"BTCUSDT","t":123456,"p":"61234.50","q":"0.00100000","T":1719580799999,"m":false,"M":true}}"#;
    const FAN_TOKEN_FRAME: &str = r#"{"stream":"psgusdt@trade","data":{"e":"trade","E":1719580800001,"s":"PSGUSDT","t":42,"p":"2.345","q":"10.0","T":1719580800000,"m":true,"M":true}}"#;
    const COMMAND_RESULT: &str = r#"{"result":null,"id":1}"#;
    const ERROR_FRAME: &str = r#"{"error":{"code":2,"msg":"Invalid request: invalid stream"}}"#;

    #[test]
    fn parses_trade_frame_into_combined() {
        let msg: BinanceWsMessage =
            serde_json::from_str(TRADE_FRAME).expect("Failed to parse trade frame");

        match msg {
            BinanceWsMessage::Combined { stream, data } => {
                assert_eq!(stream, "btcusdt@trade");
                assert_eq!(data.event_type, "trade");
                assert_eq!(data.symbol, "BTCUSDT");
            }
            other => panic!("Expected Combined variant, got {:?}", other),
        }
    }

    #[test]
    fn routes_subject_from_inner_symbol_not_stream() {
        // The subject symbol must come from `data.s` (upper-case canonical),
        // not the lower-case `stream` token.
        let msg: BinanceWsMessage =
            serde_json::from_str(FAN_TOKEN_FRAME).expect("Failed to parse fan-token frame");

        match msg {
            BinanceWsMessage::Combined { stream, data } => {
                assert_eq!(stream, "psgusdt@trade");
                assert_eq!(data.symbol, "PSGUSDT");
            }
            other => panic!("Expected Combined variant, got {:?}", other),
        }
    }

    #[test]
    fn parses_full_trade_data_fields() {
        let frame: serde_json::Value =
            serde_json::from_str(TRADE_FRAME).expect("Failed to parse frame");
        let data = frame.get("data").cloned().expect("frame must have data");
        let trade: BinanceTradeData =
            serde_json::from_value(data).expect("Failed to parse trade data");

        assert_eq!(trade.event_type, "trade");
        assert_eq!(trade.event_time, 1719580800000);
        assert_eq!(trade.symbol, "BTCUSDT");
        assert_eq!(trade.trade_id, 123456);
        assert_eq!(trade.price, "61234.50");
        assert_eq!(trade.quantity, "0.00100000");
        assert_eq!(trade.trade_time, 1719580799999);
        assert!(!trade.is_buyer_maker);
    }

    #[test]
    fn parses_command_result_frame() {
        let msg: BinanceWsMessage =
            serde_json::from_str(COMMAND_RESULT).expect("Failed to parse command result");

        match msg {
            BinanceWsMessage::CommandResult { result, id } => {
                assert!(result.is_none());
                assert_eq!(id, serde_json::json!(1));
            }
            other => panic!("Expected CommandResult variant, got {:?}", other),
        }
    }

    #[test]
    fn parses_error_frame() {
        let msg: BinanceWsMessage =
            serde_json::from_str(ERROR_FRAME).expect("Failed to parse error frame");

        match msg {
            BinanceWsMessage::Error { error } => {
                assert_eq!(error.get("code").and_then(|c| c.as_i64()), Some(2));
            }
            other => panic!("Expected Error variant, got {:?}", other),
        }
    }

    #[test]
    fn rejects_malformed_payload() {
        // Non-JSON bytes must fail to parse (the recv loop skips these).
        assert!(serde_json::from_str::<BinanceWsMessage>("not json at all").is_err());
    }

    #[test]
    fn rejects_empty_payload() {
        assert!(serde_json::from_str::<BinanceWsMessage>("").is_err());
    }
}
