//! Kraken Futures WebSocket v1 client.
//!
//! Connects to wss://futures.kraken.com/ws/v1 and manages subscriptions.
//! Key differences from spot (v2):
//! - Subscribe: {"event":"subscribe","feed":"<feed>","product_ids":["PI_XBTUSD"]}
//! - Ping: every 30s (matches spot)
//! - Symbols: PI_XBTUSD, PF_ETHUSD (no slash)

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{connect_async_with_config, tungstenite};
use tracing::{debug, info};

use super::messages::KrakenFuturesWsMessage;

pub const KRAKEN_FUTURES_WS_URL: &str = "wss://futures.kraken.com/ws/v1";

const MAX_MESSAGE_SIZE: usize = 1024 * 1024; // 1 MiB
const SUBSCRIBE_TIMEOUT_SECS: u64 = 30;
const READ_TIMEOUT_SECS: u64 = 90; // 3x ping interval
pub const PING_INTERVAL_SECS: u64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum KrakenFuturesWsError {
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Connection closed")]
    ConnectionClosed,
    #[error("Subscribe timeout")]
    SubscribeTimeout,
    #[error("Read timeout")]
    ReadTimeout,
    #[error("Server error: {0}")]
    ServerError(String),
}

pub struct KrakenFuturesWebSocket {
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl KrakenFuturesWebSocket {
    /// Connect to the Kraken Futures WebSocket endpoint.
    ///
    /// If `url` is provided, it overrides the default URL.
    pub async fn connect(url: Option<&str>) -> Result<Self, KrakenFuturesWsError> {
        let url = url.unwrap_or(KRAKEN_FUTURES_WS_URL);
        let config = tungstenite::protocol::WebSocketConfig {
            max_message_size: Some(MAX_MESSAGE_SIZE),
            ..Default::default()
        };

        info!(url = %url, "Connecting to Kraken Futures WS");
        let (ws, _) = connect_async_with_config(url, Some(config), false).await?;
        info!("Connected to Kraken Futures WS");

        Ok(Self { ws })
    }

    /// Subscribe to a feed (e.g., "trade", "ticker") for given product IDs.
    pub async fn subscribe(
        &mut self,
        feed: &str,
        product_ids: &[String],
    ) -> Result<(), KrakenFuturesWsError> {
        let msg = json!({
            "event": "subscribe",
            "feed": feed,
            "product_ids": product_ids,
        });

        debug!(feed = feed, products = ?product_ids, "Subscribing to Kraken Futures feed");
        self.ws
            .send(tungstenite::Message::Text(msg.to_string()))
            .await?;

        // Wait for subscription acknowledgment
        let ack = timeout(Duration::from_secs(SUBSCRIBE_TIMEOUT_SECS), async {
            while let Some(msg) = self.ws.next().await {
                match msg {
                    Ok(tungstenite::Message::Text(text)) => {
                        if let Ok(parsed) = serde_json::from_str::<KrakenFuturesWsMessage>(&text) {
                            match &parsed {
                                KrakenFuturesWsMessage::Subscribed { event, .. }
                                    if event == "subscribed" =>
                                {
                                    info!(feed = feed, "Subscribed to Kraken Futures feed");
                                    return Ok(());
                                }
                                KrakenFuturesWsMessage::Error { message, .. } => {
                                    let err_msg =
                                        message.as_deref().unwrap_or("unknown error");
                                    return Err(KrakenFuturesWsError::ServerError(
                                        err_msg.to_string(),
                                    ));
                                }
                                _ => continue, // Skip info/heartbeat during subscribe
                            }
                        }
                    }
                    Ok(_) => continue,
                    Err(e) => return Err(KrakenFuturesWsError::WebSocket(e)),
                }
            }
            Err(KrakenFuturesWsError::ConnectionClosed)
        })
        .await;

        match ack {
            Ok(result) => result,
            Err(_) => Err(KrakenFuturesWsError::SubscribeTimeout),
        }
    }

    /// Receive the next raw message. Returns (raw_text, parsed_message).
    pub async fn recv_raw(
        &mut self,
    ) -> Result<(String, KrakenFuturesWsMessage), KrakenFuturesWsError> {
        let msg = timeout(Duration::from_secs(READ_TIMEOUT_SECS), self.ws.next())
            .await
            .map_err(|_| KrakenFuturesWsError::ReadTimeout)?
            .ok_or(KrakenFuturesWsError::ConnectionClosed)?
            .map_err(KrakenFuturesWsError::WebSocket)?;

        match msg {
            tungstenite::Message::Text(text) => {
                let parsed: KrakenFuturesWsMessage = serde_json::from_str(&text)?;
                Ok((text, parsed))
            }
            tungstenite::Message::Ping(data) => {
                self.ws.send(tungstenite::Message::Pong(data)).await?;
                // Recurse to get next real message
                Box::pin(self.recv_raw()).await
            }
            tungstenite::Message::Close(_) => Err(KrakenFuturesWsError::ConnectionClosed),
            _ => Box::pin(self.recv_raw()).await,
        }
    }

    /// Send a ping to keep the connection alive.
    pub async fn ping(&mut self) -> Result<(), KrakenFuturesWsError> {
        // Kraken Futures uses WebSocket-level ping frames
        self.ws
            .send(tungstenite::Message::Ping(vec![]))
            .await?;
        Ok(())
    }

    /// Close the connection.
    pub async fn close(&mut self) -> Result<(), KrakenFuturesWsError> {
        self.ws.close(None).await?;
        Ok(())
    }
}
