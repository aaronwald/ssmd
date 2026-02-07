//! Kraken Futures WebSocket v1 message types.
//!
//! The Futures API uses a different protocol from spot (v2):
//! - Subscribe: {"event":"subscribe","feed":"ticker","product_ids":["PI_XBTUSD"]}
//! - Data: {"feed":"trade","product_id":"PI_XBTUSD",...}
//! - Heartbeat: {"event":"heartbeat"}
//!
//! Reference: https://docs.kraken.com/api/docs/guides/futures-websockets/

use serde::Deserialize;

/// Top-level message from the Kraken Futures WebSocket.
/// Uses `#[serde(untagged)]` — variant order matters!
/// DataMessage must come first (most specific), Heartbeat last (least specific).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum KrakenFuturesWsMessage {
    /// Channel data (trade, ticker) — has "feed" + "product_id"
    DataMessage {
        feed: String,
        product_id: String,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    /// Subscription acknowledgment — has "event": "subscribed"
    Subscribed {
        event: String,
        feed: Option<String>,
        product_ids: Option<Vec<String>>,
    },
    /// Info message on connect — has "event": "info"
    Info {
        event: String,
        version: Option<u32>,
    },
    /// Error message — has "event": "error"
    Error {
        event: String,
        message: Option<String>,
    },
    /// Heartbeat — {"event": "heartbeat"}
    Heartbeat {
        event: String,
    },
}

impl KrakenFuturesWsMessage {
    /// Returns true if this is a data message (trade or ticker)
    pub fn is_data(&self) -> bool {
        matches!(self, Self::DataMessage { .. })
    }

    /// Returns true if this is a heartbeat
    pub fn is_heartbeat(&self) -> bool {
        matches!(self, Self::Heartbeat { .. })
    }

    /// Returns true if this is an error
    pub fn is_error(&self) -> bool {
        match self {
            Self::Error { event, .. } => event == "error",
            _ => false,
        }
    }
}
