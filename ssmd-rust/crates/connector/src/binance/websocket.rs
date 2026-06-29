//! Binance combined-stream WebSocket client
//!
//! Connects to the public Binance spot market-data mirror
//! (`wss://data-stream.binance.vision`) using the **combined stream** form,
//! where the requested streams are encoded in the URL query:
//!
//! ```text
//! wss://data-stream.binance.vision/stream?streams=btcusdt@trade/ethusdt@trade
//! ```
//!
//! No authentication is required for public `@trade` streams. Because the
//! streams are requested via the URL, Binance starts delivering data
//! immediately — there is no subscribe-and-wait handshake to perform.
//!
//! Per the architectural rules (`defensive-coding`): on any WebSocket error or
//! a server-side close, the caller must crash the pod (K8s restarts it). There
//! is no reconnect-and-hope.

use crate::binance::messages::BinanceWsMessage;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::{info, trace, warn};

/// Binance public spot market-data WebSocket base URL (the `.vision` mirror,
/// which — unlike the global `stream.binance.com` — is reachable from US/GCP
/// egress without a 451 geo-block).
pub const BINANCE_WS_URL: &str = "wss://data-stream.binance.vision";

#[derive(Error, Debug)]
pub enum BinanceWebSocketError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Subscription failed: {0}")]
    SubscriptionFailed(String),
}

/// Binance combined-stream WebSocket client.
pub struct BinanceWebSocket {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl BinanceWebSocket {
    /// Max WebSocket message size: 1 MiB (a single `@trade` frame is <1 KB).
    const MAX_MESSAGE_SIZE: usize = 1_048_576;

    /// Read timeout in seconds — Binance sends a ping every ~3 min and trades
    /// flow continuously for liquid symbols, so a 120s gap signals a dead link.
    const READ_TIMEOUT_SECS: u64 = 120;

    /// Build the combined-stream URL for the given symbols.
    ///
    /// Symbols are lower-cased and suffixed with `@trade`, then joined with `/`
    /// into the `streams` query parameter:
    /// `{base}/stream?streams=btcusdt@trade/ethusdt@trade`.
    ///
    /// Fails loud if `symbols` is empty — starting with no subscriptions is an
    /// unrecoverable misconfiguration. Returns a plain `String` error (kept
    /// small on purpose so this sync helper does not carry the large WS error
    /// variant); [`connect`](Self::connect) maps it into a
    /// [`BinanceWebSocketError`].
    pub(crate) fn build_combined_url(
        base_url: &str,
        symbols: &[String],
    ) -> Result<String, String> {
        let trimmed: Vec<&str> = symbols
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if trimmed.is_empty() {
            return Err("symbols list must not be empty".to_string());
        }

        let streams = trimmed
            .iter()
            .map(|s| format!("{}@trade", s.to_lowercase()))
            .collect::<Vec<_>>()
            .join("/");

        let base = base_url.trim_end_matches('/');
        Ok(format!("{}/stream?streams={}", base, streams))
    }

    /// Connect to the Binance combined stream for the given symbols.
    ///
    /// `base_url` overrides the default [`BINANCE_WS_URL`] when provided (e.g.
    /// from the feed ConfigMap). The streams are encoded into the connect URL,
    /// so Binance begins delivering trades as soon as the socket is open.
    pub async fn connect(
        base_url: Option<&str>,
        symbols: &[String],
    ) -> Result<Self, BinanceWebSocketError> {
        let base = base_url.unwrap_or(BINANCE_WS_URL);
        let url = Self::build_combined_url(base, symbols)
            .map_err(BinanceWebSocketError::SubscriptionFailed)?;
        info!(
            base = %base,
            symbol_count = symbols.len(),
            "Connecting to Binance combined stream"
        );

        let config = WebSocketConfig {
            max_message_size: Some(Self::MAX_MESSAGE_SIZE),
            max_frame_size: Some(Self::MAX_MESSAGE_SIZE),
            ..Default::default()
        };

        let (ws, response) = connect_async_with_config(&url, Some(config), false).await?;
        info!(status = ?response.status(), "Binance WebSocket connected");

        Ok(Self { ws })
    }

    /// Receive the next message with raw text.
    /// Returns `(raw_json, parsed_message)`.
    ///
    /// WS-level `Ping` frames are answered with a `Pong` inline. Frames that
    /// fail to parse are logged and skipped (the loop continues) so a single
    /// malformed payload never crashes the connector. A server `Close` or a
    /// read timeout surfaces as an error for the caller to crash on.
    pub async fn recv_raw(&mut self) -> Result<(String, BinanceWsMessage), BinanceWebSocketError> {
        loop {
            let recv_result = tokio::time::timeout(
                Duration::from_secs(Self::READ_TIMEOUT_SECS),
                self.ws.next(),
            )
            .await;

            match recv_result {
                Err(_) => {
                    warn!(
                        timeout_secs = Self::READ_TIMEOUT_SECS,
                        "Binance WebSocket read timeout"
                    );
                    return Err(BinanceWebSocketError::Connection(format!(
                        "Read timeout after {} seconds",
                        Self::READ_TIMEOUT_SECS
                    )));
                }
                Ok(Some(Ok(Message::Text(text)))) => {
                    match serde_json::from_str::<BinanceWsMessage>(&text) {
                        Ok(msg) => {
                            trace!(msg = %text, "Received Binance message");
                            return Ok((text, msg));
                        }
                        Err(e) => {
                            warn!(error = %e, text = %text, "Failed to parse Binance message");
                            continue;
                        }
                    }
                }
                Ok(Some(Ok(Message::Ping(data)))) => {
                    trace!("Received WS ping, sending pong");
                    self.ws.send(Message::Pong(data)).await?;
                }
                Ok(Some(Ok(Message::Close(frame)))) => {
                    info!(frame = ?frame, "Binance WebSocket closed");
                    return Err(BinanceWebSocketError::ConnectionClosed);
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(e.into()),
                Ok(None) => return Err(BinanceWebSocketError::ConnectionClosed),
            }
        }
    }

    /// Send a WS-level ping frame for keepalive.
    ///
    /// Binance will reply with a pong; this also refreshes the connector's
    /// activity tracker during quiet periods.
    pub async fn ping(&mut self) -> Result<(), BinanceWebSocketError> {
        self.ws.send(Message::Ping(Vec::new())).await?;
        Ok(())
    }

    /// Close the connection gracefully.
    pub async fn close(&mut self) -> Result<(), BinanceWebSocketError> {
        self.ws.close(None).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_error_display() {
        let err = BinanceWebSocketError::ConnectionClosed;
        assert_eq!(format!("{}", err), "Connection closed");

        let err = BinanceWebSocketError::SubscriptionFailed("empty".to_string());
        assert_eq!(format!("{}", err), "Subscription failed: empty");
    }

    #[test]
    fn test_url_constant() {
        assert!(BINANCE_WS_URL.starts_with("wss://"));
        assert!(BINANCE_WS_URL.contains("data-stream.binance.vision"));
    }

    #[test]
    fn build_combined_url_single_symbol() {
        let url = BinanceWebSocket::build_combined_url(BINANCE_WS_URL, &["BTCUSDT".to_string()])
            .expect("should build url");
        assert_eq!(
            url,
            "wss://data-stream.binance.vision/stream?streams=btcusdt@trade"
        );
    }

    #[test]
    fn build_combined_url_multiple_symbols_lowercased() {
        let url = BinanceWebSocket::build_combined_url(
            BINANCE_WS_URL,
            &["BTCUSDT".to_string(), "ETHUSDT".to_string(), "PSGUSDT".to_string()],
        )
        .expect("should build url");
        assert_eq!(
            url,
            "wss://data-stream.binance.vision/stream?streams=btcusdt@trade/ethusdt@trade/psgusdt@trade"
        );
    }

    #[test]
    fn build_combined_url_trims_trailing_slash_on_base() {
        let url = BinanceWebSocket::build_combined_url(
            "wss://data-stream.binance.vision/",
            &["BTCUSDT".to_string()],
        )
        .expect("should build url");
        assert_eq!(
            url,
            "wss://data-stream.binance.vision/stream?streams=btcusdt@trade"
        );
    }

    #[test]
    fn build_combined_url_skips_blank_symbols() {
        let url = BinanceWebSocket::build_combined_url(
            BINANCE_WS_URL,
            &["BTCUSDT".to_string(), "  ".to_string(), "ETHUSDT".to_string()],
        )
        .expect("should build url");
        assert_eq!(
            url,
            "wss://data-stream.binance.vision/stream?streams=btcusdt@trade/ethusdt@trade"
        );
    }

    #[test]
    fn build_combined_url_empty_symbols_fails() {
        let result = BinanceWebSocket::build_combined_url(BINANCE_WS_URL, &[]);
        assert!(result.is_err());

        let result = BinanceWebSocket::build_combined_url(BINANCE_WS_URL, &["  ".to_string()]);
        assert!(result.is_err());
    }
}
