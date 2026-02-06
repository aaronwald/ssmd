//! Polymarket CLOB WebSocket client
//!
//! Handles connection, subscription, and message receiving for Polymarket's CLOB WebSocket API.
//! No authentication required for the public market data channel.
//!
//! Key differences from Kraken/Kalshi:
//! - Keepalive: raw text "PING" every 10 seconds (not JSON, not WS ping frames)
//! - Subscribes by asset_id (token ID), not symbol name
//! - Max 500 instruments per connection
//! - Known instability: streams may stop after ~20 minutes

use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, info, trace, warn};

/// Polymarket CLOB WebSocket URL
pub const POLYMARKET_WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/";

/// Max instruments per WebSocket connection
pub const MAX_INSTRUMENTS_PER_CONNECTION: usize = 500;

#[derive(Error, Debug)]
pub enum PolymarketWebSocketError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Connection closed")]
    ConnectionClosed,
}

/// Polymarket CLOB WebSocket client
pub struct PolymarketWebSocket {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl PolymarketWebSocket {
    /// Max WebSocket message size: 2 MiB (book snapshots can be large with many price levels)
    const MAX_MESSAGE_SIZE: usize = 2_097_152;

    /// Read timeout: 120 seconds (same as Kraken/Kalshi)
    const READ_TIMEOUT_SECS: u64 = 120;

    /// Connect to Polymarket CLOB WebSocket (no authentication needed for market channel)
    pub async fn connect() -> Result<Self, PolymarketWebSocketError> {
        info!(url = %POLYMARKET_WS_URL, "Connecting to Polymarket WebSocket");

        let config = WebSocketConfig {
            max_message_size: Some(Self::MAX_MESSAGE_SIZE),
            max_frame_size: Some(Self::MAX_MESSAGE_SIZE),
            ..Default::default()
        };

        let (ws, response) =
            connect_async_with_config(POLYMARKET_WS_URL, Some(config), false).await?;

        info!(status = ?response.status(), "Polymarket WebSocket connected");

        Ok(Self { ws })
    }

    /// Subscribe to the market channel for the given asset IDs (token IDs).
    ///
    /// Sends: `{"assets_ids": [...], "type": "market", "custom_feature_enabled": true}`
    ///
    /// Polymarket does NOT send subscription confirmations like Kraken/Kalshi.
    /// Instead, it immediately starts sending `book` snapshots for subscribed instruments.
    pub async fn subscribe(
        &mut self,
        asset_ids: &[String],
    ) -> Result<(), PolymarketWebSocketError> {
        if asset_ids.is_empty() {
            return Ok(());
        }

        if asset_ids.len() > MAX_INSTRUMENTS_PER_CONNECTION {
            warn!(
                count = asset_ids.len(),
                max = MAX_INSTRUMENTS_PER_CONNECTION,
                "Subscribing to more instruments than connection limit"
            );
        }

        let subscribe_msg = serde_json::json!({
            "assets_ids": asset_ids,
            "type": "market",
            "custom_feature_enabled": true
        });

        let msg = serde_json::to_string(&subscribe_msg)?;
        debug!(count = asset_ids.len(), "Sending Polymarket subscribe command");

        self.ws.send(Message::Text(msg)).await?;

        info!(
            count = asset_ids.len(),
            "Polymarket subscription sent for market channel"
        );

        Ok(())
    }

    /// Dynamically subscribe to additional asset IDs on an existing connection.
    pub async fn subscribe_additional(
        &mut self,
        asset_ids: &[String],
    ) -> Result<(), PolymarketWebSocketError> {
        if asset_ids.is_empty() {
            return Ok(());
        }

        let msg = serde_json::json!({
            "assets_ids": asset_ids,
            "operation": "subscribe"
        });

        let text = serde_json::to_string(&msg)?;
        debug!(count = asset_ids.len(), "Sending dynamic subscribe");
        self.ws.send(Message::Text(text)).await?;
        Ok(())
    }

    /// Receive the next raw text message from the WebSocket.
    /// Returns the raw JSON string for pass-through to NATS.
    /// Handles WS-level ping/pong frames automatically.
    pub async fn recv_raw(&mut self) -> Result<String, PolymarketWebSocketError> {
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
                        "Polymarket WebSocket read timeout"
                    );
                    return Err(PolymarketWebSocketError::Connection(format!(
                        "Read timeout after {} seconds",
                        Self::READ_TIMEOUT_SECS
                    )));
                }
                Ok(Some(Ok(Message::Text(text)))) => {
                    trace!(len = text.len(), "Received Polymarket message");
                    return Ok(text);
                }
                Ok(Some(Ok(Message::Ping(data)))) => {
                    trace!("Received WS ping, sending pong");
                    self.ws.send(Message::Pong(data)).await?;
                }
                Ok(Some(Ok(Message::Close(frame)))) => {
                    info!(frame = ?frame, "Polymarket WebSocket closed");
                    return Err(PolymarketWebSocketError::ConnectionClosed);
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(e.into()),
                Ok(None) => return Err(PolymarketWebSocketError::ConnectionClosed),
            }
        }
    }

    /// Send app-level ping.
    ///
    /// Polymarket uses a raw text "PING" string (not JSON, not WS-level ping frames).
    /// Must be sent every 10 seconds.
    pub async fn ping(&mut self) -> Result<(), PolymarketWebSocketError> {
        self.ws.send(Message::Text("PING".to_string())).await?;
        Ok(())
    }

    /// Close the connection gracefully
    pub async fn close(&mut self) -> Result<(), PolymarketWebSocketError> {
        self.ws.close(None).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_error_display() {
        let err = PolymarketWebSocketError::ConnectionClosed;
        assert_eq!(format!("{}", err), "Connection closed");

        let err = PolymarketWebSocketError::Connection("timeout".to_string());
        assert_eq!(format!("{}", err), "Connection error: timeout");
    }

    #[test]
    fn test_url_constant() {
        assert!(POLYMARKET_WS_URL.starts_with("wss://"));
        assert!(POLYMARKET_WS_URL.contains("polymarket.com"));
    }

    #[test]
    fn test_max_instruments_constant() {
        assert_eq!(MAX_INSTRUMENTS_PER_CONNECTION, 500);
    }
}
