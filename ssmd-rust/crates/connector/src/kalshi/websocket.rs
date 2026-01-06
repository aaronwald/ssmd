//! Kalshi WebSocket client
//!
//! Handles connection, authentication, subscription, and message receiving.

use crate::kalshi::auth::{AuthError, KalshiCredentials};
use crate::kalshi::messages::{WsCommand, WsMessage, WsParams};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{http::Request, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, info, trace, warn};

/// Result of a batch subscription operation
#[derive(Debug, Default)]
pub struct SubscriptionResult {
    pub successful: usize,
    pub failed: usize,
    pub failed_tickers: Vec<String>,
}

/// Production WebSocket URL
pub const KALSHI_WS_URL: &str = "wss://api.elections.kalshi.com/trade-api/ws/v2";

/// Demo WebSocket URL
pub const KALSHI_WS_DEMO_URL: &str = "wss://demo-api.kalshi.co/trade-api/ws/v2";

#[derive(Error, Debug)]
pub enum WebSocketError {
    #[error("Authentication error: {0}")]
    Auth(#[from] AuthError),

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

/// Kalshi WebSocket client
pub struct KalshiWebSocket {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    command_id: u64,
    subscribed_markets: Vec<String>,
}

impl KalshiWebSocket {
    /// Connect to Kalshi WebSocket with authentication
    pub async fn connect(
        credentials: &KalshiCredentials,
        use_demo: bool,
    ) -> Result<Self, WebSocketError> {
        let url = if use_demo {
            KALSHI_WS_DEMO_URL
        } else {
            KALSHI_WS_URL
        };

        let (timestamp, signature) = credentials.sign_websocket_request()?;

        let url_without_scheme = url.replace("wss://", "");
        let host = url_without_scheme
            .split('/')
            .next()
            .unwrap_or("api.elections.kalshi.com");

        let request = Request::builder()
            .uri(url)
            .header("KALSHI-ACCESS-KEY", &credentials.api_key)
            .header("KALSHI-ACCESS-SIGNATURE", &signature)
            .header("KALSHI-ACCESS-TIMESTAMP", &timestamp)
            .header("Host", host)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|e| WebSocketError::Connection(e.to_string()))?;

        info!(url = %url, "Connecting to Kalshi WebSocket");

        let (ws, response) = connect_async(request).await?;

        info!(status = ?response.status(), "WebSocket connected");

        Ok(Self {
            ws,
            command_id: 0,
            subscribed_markets: Vec::new(),
        })
    }

    /// Subscribe to ticker updates for all markets
    pub async fn subscribe_ticker(&mut self) -> Result<(), WebSocketError> {
        self.command_id += 1;
        let cmd = WsCommand {
            id: self.command_id,
            cmd: "subscribe".to_string(),
            params: WsParams {
                channels: vec!["ticker".to_string()],
                market_ticker: None,
                market_tickers: None,
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        debug!(cmd = %msg, "Sending subscribe command");

        self.ws.send(Message::Text(msg)).await?;

        self.wait_for_subscription(self.command_id).await
    }

    /// Subscribe to trades for a specific market
    pub async fn subscribe_trades(&mut self, market_ticker: &str) -> Result<(), WebSocketError> {
        self.command_id += 1;
        let cmd = WsCommand {
            id: self.command_id,
            cmd: "subscribe".to_string(),
            params: WsParams {
                channels: vec!["trade".to_string()],
                market_ticker: Some(market_ticker.to_string()),
                market_tickers: None,
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        debug!(cmd = %msg, market = %market_ticker, "Sending subscribe command");

        self.ws.send(Message::Text(msg)).await?;
        self.subscribed_markets.push(market_ticker.to_string());

        self.wait_for_subscription(self.command_id).await
    }

    /// Subscribe to all trade executions globally
    pub async fn subscribe_all_trades(&mut self) -> Result<(), WebSocketError> {
        self.command_id += 1;
        let cmd = WsCommand {
            id: self.command_id,
            cmd: "subscribe".to_string(),
            params: WsParams {
                channels: vec!["trade".to_string()],
                market_ticker: None,
                market_tickers: None,
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        debug!(cmd = %msg, "Subscribing to all trades");

        self.ws.send(Message::Text(msg)).await?;
        self.wait_for_subscription(self.command_id).await
    }

    /// Subscribe to orderbook for a specific market
    pub async fn subscribe_orderbook(&mut self, market_ticker: &str) -> Result<(), WebSocketError> {
        self.command_id += 1;
        let cmd = WsCommand {
            id: self.command_id,
            cmd: "subscribe".to_string(),
            params: WsParams {
                channels: vec!["orderbook_delta".to_string()],
                market_ticker: Some(market_ticker.to_string()),
                market_tickers: None,
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        self.ws.send(Message::Text(msg)).await?;

        self.wait_for_subscription(self.command_id).await
    }

    /// Maximum markets per subscription (Kalshi limit)
    pub const MAX_MARKETS_PER_SUBSCRIPTION: usize = 500;

    /// Subscribe to a channel for multiple markets
    ///
    /// Kalshi supports up to 500 markets per subscription.
    /// If you need more markets, deploy multiple connectors with different filters.
    pub async fn subscribe_markets(
        &mut self,
        channel: &str,
        tickers: &[String],
    ) -> Result<(), WebSocketError> {
        if tickers.len() > Self::MAX_MARKETS_PER_SUBSCRIPTION {
            return Err(WebSocketError::SubscriptionFailed(format!(
                "Too many markets ({}) - Kalshi limit is {}. Deploy multiple connectors with different category filters.",
                tickers.len(),
                Self::MAX_MARKETS_PER_SUBSCRIPTION
            )));
        }

        self.command_id += 1;
        let cmd = WsCommand {
            id: self.command_id,
            cmd: "subscribe".to_string(),
            params: WsParams {
                channels: vec![channel.to_string()],
                market_ticker: None,
                market_tickers: Some(tickers.to_vec()),
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        info!(
            channel = %channel,
            markets = tickers.len(),
            "Subscribing to channel"
        );

        self.ws.send(Message::Text(msg)).await?;

        self.wait_for_subscription(self.command_id).await?;

        info!(
            channel = %channel,
            cmd_id = self.command_id,
            markets = tickers.len(),
            "Subscription confirmed by Kalshi"
        );
        self.subscribed_markets.extend(tickers.iter().cloned());

        Ok(())
    }

    /// Wait for subscription confirmation
    async fn wait_for_subscription(&mut self, expected_id: u64) -> Result<(), WebSocketError> {
        let timeout = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(msg) = self.ws.next().await {
                match msg? {
                    Message::Text(text) => {
                        if let Ok(WsMessage::Subscribed { id }) = serde_json::from_str(&text) {
                            if id == expected_id {
                                info!(id, "Subscription confirmed");
                                return Ok(());
                            }
                        }
                    }
                    Message::Close(_) => return Err(WebSocketError::ConnectionClosed),
                    _ => continue,
                }
            }
            Err(WebSocketError::ConnectionClosed)
        });

        timeout
            .await
            .map_err(|_| WebSocketError::SubscriptionFailed("Timeout waiting for confirmation".into()))?
    }

    /// Receive the next message with raw text
    /// Returns (raw_json, parsed_message) for raw data capture
    pub async fn recv_raw(&mut self) -> Result<(String, WsMessage), WebSocketError> {
        loop {
            match self.ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<WsMessage>(&text) {
                        Ok(msg) => {
                            trace!(msg = %text, "Received message");
                            return Ok((text, msg));
                        }
                        Err(e) => {
                            warn!(error = %e, text = %text, "Failed to parse message");
                            continue;
                        }
                    }
                }
                Some(Ok(Message::Ping(data))) => {
                    trace!("Received ping, sending pong");
                    self.ws.send(Message::Pong(data)).await?;
                }
                Some(Ok(Message::Close(frame))) => {
                    info!(frame = ?frame, "WebSocket closed");
                    return Err(WebSocketError::ConnectionClosed);
                }
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(e.into()),
                None => return Err(WebSocketError::ConnectionClosed),
            }
        }
    }

    /// Send a ping to keep connection alive
    pub async fn ping(&mut self) -> Result<(), WebSocketError> {
        self.ws.send(Message::Ping(vec![])).await?;
        Ok(())
    }

    /// Close the connection gracefully
    pub async fn close(&mut self) -> Result<(), WebSocketError> {
        self.ws.close(None).await?;
        Ok(())
    }

    /// Get list of subscribed markets
    pub fn subscribed_markets(&self) -> &[String] {
        &self.subscribed_markets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_error_display() {
        let err = WebSocketError::ConnectionClosed;
        assert_eq!(format!("{}", err), "Connection closed");

        let err = WebSocketError::SubscriptionFailed("timeout".to_string());
        assert_eq!(format!("{}", err), "Subscription failed: timeout");
    }

    #[test]
    fn test_url_constants() {
        assert!(KALSHI_WS_URL.starts_with("wss://"));
        assert!(KALSHI_WS_DEMO_URL.starts_with("wss://"));
        assert!(KALSHI_WS_URL.contains("kalshi.com"));
        assert!(KALSHI_WS_DEMO_URL.contains("kalshi.co"));
    }
}
