//! Polygon.io ("massive") WebSocket transport layer
//!
//! Handles connection to the delayed Polygon cluster, authentication via
//! auth frame, subscribe to trade+quote channels, and recv loop.

use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::{info, warn};

use ssmd_middleware::now_tsc;

/// Delayed Polygon.io stocks cluster endpoint.
/// The realtime cluster (`socket.polygon.io`) requires a paid plan.
pub const MASSIVE_WS_DELAYED_URL: &str = "wss://delayed.polygon.io/stocks";

/// Errors from the Polygon.io WebSocket transport.
#[derive(Debug, Error)]
pub enum MassiveWsError {
    #[error("ws error: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("auth failed: {0}")]
    Auth(String),
}

/// Build the auth JSON frame for the Polygon.io auth handshake.
///
/// Polygon authenticates by sending this frame as the first text message after
/// connect, then waits for `{"ev":"status","status":"auth_success"}`.
pub(crate) fn auth_frame(api_key: &str) -> String {
    format!(r#"{{"action":"auth","params":"{api_key}"}}"#)
}

/// Build the subscribe JSON frame for trade + quote channels.
///
/// Each symbol produces two channels: `T.<sym>` (trades) and `Q.<sym>` (quotes).
/// Channels are joined as a comma-separated params list.
///
/// Example for `["AAPL", "SPY"]`:
/// `{"action":"subscribe","params":"T.AAPL,Q.AAPL,T.SPY,Q.SPY"}`
pub(crate) fn subscribe_frame(symbols: &[String]) -> String {
    let params = symbols
        .iter()
        .flat_map(|s| [format!("T.{s}"), format!("Q.{s}")])
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"action":"subscribe","params":"{params}"}}"#)
}

/// Polygon.io WebSocket client for the delayed equities cluster.
pub struct MassiveWebSocket {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl MassiveWebSocket {
    /// Max WebSocket message size: 4 MiB
    const MAX_MESSAGE_SIZE: usize = 4 * 1_048_576;

    /// Timeout waiting for auth_success from Polygon.io.
    const AUTH_TIMEOUT_SECS: u64 = 30;

    /// Connect to the delayed Polygon.io WebSocket endpoint.
    ///
    /// If `url` is `Some`, it overrides the default `MASSIVE_WS_DELAYED_URL`.
    pub async fn connect(url: Option<&str>) -> Result<Self, MassiveWsError> {
        let url = url.unwrap_or(MASSIVE_WS_DELAYED_URL);
        let config = WebSocketConfig {
            max_message_size: Some(Self::MAX_MESSAGE_SIZE),
            max_frame_size: Some(Self::MAX_MESSAGE_SIZE),
            ..Default::default()
        };
        let (ws, response) = connect_async_with_config(url, Some(config), false).await?;
        info!(status = ?response.status(), "Massive WebSocket connected");
        Ok(Self { ws })
    }

    /// Send the auth frame, then read frames until `auth_success`.
    ///
    /// Returns `Err(MassiveWsError::Auth)` on:
    /// - empty `api_key`
    /// - `auth_failed` status from Polygon
    /// - connection closed before auth completes
    /// - timeout waiting for auth_success
    pub async fn authenticate(&mut self, api_key: &str) -> Result<(), MassiveWsError> {
        if api_key.is_empty() {
            return Err(MassiveWsError::Auth("api_key must not be empty".into()));
        }

        self.ws.send(Message::Text(auth_frame(api_key))).await?;

        let auth_result = tokio::time::timeout(
            Duration::from_secs(Self::AUTH_TIMEOUT_SECS),
            async {
                while let Some(frame) = self.ws.next().await {
                    let bytes = match frame? {
                        Message::Text(t) => t.into_bytes(),
                        Message::Binary(b) => b,
                        _ => continue,
                    };
                    for m in crate::massive::messages::parse_frame(&bytes) {
                        if let crate::massive::messages::MassiveMessage::Status(s) = m {
                            match s.status.as_str() {
                                "auth_success" => return Ok(()),
                                "auth_failed" => {
                                    return Err(MassiveWsError::Auth(s.message))
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(MassiveWsError::Auth(
                    "connection closed before auth_success".into(),
                ))
            },
        )
        .await;

        match auth_result {
            Ok(result) => result,
            Err(_elapsed) => Err(MassiveWsError::Auth(format!(
                "auth timeout after {}s",
                Self::AUTH_TIMEOUT_SECS
            ))),
        }
    }

    /// Subscribe to `T.<sym>` and `Q.<sym>` channels for each symbol.
    ///
    /// Returns `Err` if `symbols` is empty (nothing to subscribe to).
    pub async fn subscribe(&mut self, symbols: &[String]) -> Result<(), MassiveWsError> {
        if symbols.is_empty() {
            return Err(MassiveWsError::Auth(
                "subscribe: symbols list must not be empty".into(),
            ));
        }
        self.ws
            .send(Message::Text(subscribe_frame(symbols)))
            .await?;
        Ok(())
    }

    /// Receive the next market data frame.
    ///
    /// Returns `Some((tsc_timestamp, raw_bytes))` for text/binary frames, or
    /// `None` on a clean close. WebSocket errors are logged and cause `None`
    /// to be returned so the caller can reconnect. Ping/pong frames are skipped.
    pub async fn recv(&mut self) -> Option<(u64, Vec<u8>)> {
        while let Some(frame) = self.ws.next().await {
            match frame {
                Ok(Message::Text(t)) => return Some((now_tsc(), t.into_bytes())),
                Ok(Message::Binary(b)) => return Some((now_tsc(), b)),
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => continue,
                Ok(Message::Close(_)) => return None,
                Ok(_) => continue,
                Err(e) => {
                    warn!(error = %e, "Massive WebSocket recv error");
                    return None;
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_frame_is_correct() {
        let f = auth_frame("KEY123");
        assert_eq!(f, r#"{"action":"auth","params":"KEY123"}"#);
    }

    #[test]
    fn subscribe_frame_lists_trade_and_quote_channels() {
        let f = subscribe_frame(&["AAPL".to_string(), "SPY".to_string()]);
        assert_eq!(f, r#"{"action":"subscribe","params":"T.AAPL,Q.AAPL,T.SPY,Q.SPY"}"#);
    }
}
