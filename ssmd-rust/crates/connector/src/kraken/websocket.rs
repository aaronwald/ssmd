//! Kraken v2 WebSocket client
//!
//! Handles connection, subscription, and message receiving for Kraken's public v2 WebSocket API.
//! No authentication required for public channels (ticker, trade).

use crate::kraken::messages::KrakenWsMessage;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, info, trace, warn};

/// Kraken v2 public WebSocket URL
pub const KRAKEN_WS_URL: &str = "wss://ws.kraken.com/v2";

#[derive(Error, Debug)]
pub enum KrakenWebSocketError {
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

/// Kraken v2 WebSocket client
pub struct KrakenWebSocket {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl KrakenWebSocket {
    /// Connect to Kraken v2 WebSocket (no authentication needed for public channels)
    pub async fn connect() -> Result<Self, KrakenWebSocketError> {
        info!(url = %KRAKEN_WS_URL, "Connecting to Kraken WebSocket v2");

        let (ws, response) = connect_async(KRAKEN_WS_URL).await?;

        info!(status = ?response.status(), "Kraken WebSocket connected");

        Ok(Self { ws })
    }

    /// Timeout for subscription confirmation
    const SUBSCRIPTION_TIMEOUT_SECS: u64 = 30;

    /// Subscribe to a channel for the given symbols
    ///
    /// Sends: `{"method":"subscribe","params":{"channel":"<channel>","symbol":["BTC/USD","ETH/USD"]}}`
    pub async fn subscribe(
        &mut self,
        channel: &str,
        symbols: &[String],
    ) -> Result<(), KrakenWebSocketError> {
        let subscribe_msg = serde_json::json!({
            "method": "subscribe",
            "params": {
                "channel": channel,
                "symbol": symbols,
            }
        });

        let msg = serde_json::to_string(&subscribe_msg)?;
        debug!(cmd = %msg, "Sending Kraken subscribe command");

        self.ws.send(Message::Text(msg)).await?;

        // Wait for subscription confirmation
        self.wait_for_subscription(channel).await
    }

    /// Wait for subscription confirmation from Kraken
    async fn wait_for_subscription(
        &mut self,
        channel: &str,
    ) -> Result<(), KrakenWebSocketError> {
        let timeout = tokio::time::timeout(
            Duration::from_secs(Self::SUBSCRIPTION_TIMEOUT_SECS),
            async {
                while let Some(msg) = self.ws.next().await {
                    match msg? {
                        Message::Text(text) => {
                            match serde_json::from_str::<KrakenWsMessage>(&text) {
                                Ok(KrakenWsMessage::SubscriptionResult {
                                    success, ..
                                }) => {
                                    if success {
                                        info!(channel = %channel, "Kraken subscription confirmed");
                                        return Ok(());
                                    } else {
                                        return Err(KrakenWebSocketError::SubscriptionFailed(
                                            format!("Subscription to {} failed", channel),
                                        ));
                                    }
                                }
                                Ok(KrakenWsMessage::Heartbeat { .. }) => {
                                    trace!("Received heartbeat while waiting for subscription");
                                    continue;
                                }
                                Ok(_) => {
                                    debug!(raw = %text, "Received non-subscription message while waiting");
                                    continue;
                                }
                                Err(e) => {
                                    warn!(error = %e, raw = %text, "Failed to parse message while waiting for subscription");
                                    continue;
                                }
                            }
                        }
                        Message::Close(_) => return Err(KrakenWebSocketError::ConnectionClosed),
                        _ => continue,
                    }
                }
                Err(KrakenWebSocketError::ConnectionClosed)
            },
        );

        timeout
            .await
            .map_err(|_| {
                warn!(
                    channel = %channel,
                    timeout_secs = Self::SUBSCRIPTION_TIMEOUT_SECS,
                    "Kraken subscription timeout"
                );
                KrakenWebSocketError::SubscriptionFailed("Timeout waiting for confirmation".into())
            })?
    }

    /// Read timeout in seconds
    const READ_TIMEOUT_SECS: u64 = 120;

    /// Receive the next message with raw text
    /// Returns (raw_json, parsed_message)
    pub async fn recv_raw(&mut self) -> Result<(String, KrakenWsMessage), KrakenWebSocketError> {
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
                        "Kraken WebSocket read timeout"
                    );
                    return Err(KrakenWebSocketError::Connection(format!(
                        "Read timeout after {} seconds",
                        Self::READ_TIMEOUT_SECS
                    )));
                }
                Ok(Some(Ok(Message::Text(text)))) => {
                    match serde_json::from_str::<KrakenWsMessage>(&text) {
                        Ok(msg) => {
                            trace!(msg = %text, "Received Kraken message");
                            return Ok((text, msg));
                        }
                        Err(e) => {
                            warn!(error = %e, text = %text, "Failed to parse Kraken message");
                            continue;
                        }
                    }
                }
                Ok(Some(Ok(Message::Ping(data)))) => {
                    trace!("Received WS ping, sending pong");
                    self.ws.send(Message::Pong(data)).await?;
                }
                Ok(Some(Ok(Message::Close(frame)))) => {
                    info!(frame = ?frame, "Kraken WebSocket closed");
                    return Err(KrakenWebSocketError::ConnectionClosed);
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(e.into()),
                Ok(None) => return Err(KrakenWebSocketError::ConnectionClosed),
            }
        }
    }

    /// Send app-level ping (not WS-level ping frame)
    ///
    /// Kraken v2 uses `{"method":"ping"}` for application-level keepalive.
    pub async fn ping(&mut self) -> Result<(), KrakenWebSocketError> {
        let ping_msg = serde_json::json!({"method": "ping"});
        let msg = serde_json::to_string(&ping_msg)?;
        self.ws.send(Message::Text(msg)).await?;
        Ok(())
    }

    /// Close the connection gracefully
    pub async fn close(&mut self) -> Result<(), KrakenWebSocketError> {
        self.ws.close(None).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_error_display() {
        let err = KrakenWebSocketError::ConnectionClosed;
        assert_eq!(format!("{}", err), "Connection closed");

        let err = KrakenWebSocketError::SubscriptionFailed("timeout".to_string());
        assert_eq!(format!("{}", err), "Subscription failed: timeout");
    }

    #[test]
    fn test_url_constant() {
        assert!(KRAKEN_WS_URL.starts_with("wss://"));
        assert!(KRAKEN_WS_URL.contains("kraken.com"));
    }
}
