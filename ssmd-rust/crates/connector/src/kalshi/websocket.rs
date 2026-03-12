//! Kalshi WebSocket client
//!
//! Handles connection, authentication, subscription, and message receiving.

use crate::kalshi::auth::{AuthError, KalshiCredentials};
use crate::kalshi::messages::{WsCommand, WsMessage, WsParams};
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
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

/// Maximum markets per WebSocket subscription (Kalshi limit)
pub const MAX_MARKETS_PER_SUBSCRIPTION: usize = 256;

/// Production WebSocket URL
pub const KALSHI_WS_URL: &str = "wss://api.kalshi.com/trade-api/ws/v2";

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

    /// Message received but failed to parse — raw JSON preserved
    #[error("Parse error: {error}")]
    ParseFailed {
        error: String,
        raw_json: String,
    },
}

/// Tracks subscription IDs (sids) returned by Kalshi for each subscribe command.
/// Maps sids to their (channel, tickers) and tickers back to their sids.
#[derive(Debug, Default)]
pub struct SidTracker {
    /// sid -> (channel_name, Vec of tickers)
    sid_to_batch: HashMap<u64, (String, Vec<String>)>,
    /// ticker -> Vec of sids (one sid per channel the ticker is subscribed on)
    ticker_to_sids: HashMap<String, Vec<u64>>,
}

impl SidTracker {
    /// Create a new empty SidTracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a subscription: map sid to (channel, tickers) and each ticker back to this sid
    pub fn record_subscription(&mut self, sid: u64, channel: &str, tickers: &[String]) {
        self.sid_to_batch
            .insert(sid, (channel.to_string(), tickers.to_vec()));
        for ticker in tickers {
            self.ticker_to_sids
                .entry(ticker.clone())
                .or_default()
                .push(sid);
        }
    }

    /// Get all sids that include the given ticker
    pub fn sids_for_ticker(&self, ticker: &str) -> Vec<u64> {
        self.ticker_to_sids
            .get(ticker)
            .cloned()
            .unwrap_or_default()
    }

    /// Get channel and tickers for a given sid
    pub fn tickers_for_sid(&self, sid: u64) -> Option<(&str, &[String])> {
        self.sid_to_batch
            .get(&sid)
            .map(|(ch, tks)| (ch.as_str(), tks.as_slice()))
    }

    /// Remove a market from all batches. Returns affected (sid, channel, remaining_tickers).
    ///
    /// Removes the ticker from `ticker_to_sids` entirely. For each affected sid,
    /// removes the ticker from that batch. If the batch becomes empty, removes the sid entry.
    pub fn remove_market(&mut self, ticker: &str) -> Vec<(u64, String, Vec<String>)> {
        let sids = match self.ticker_to_sids.remove(ticker) {
            Some(sids) => sids,
            None => return Vec::new(),
        };

        let mut result = Vec::new();
        for sid in sids {
            if let Some((channel, batch)) = self.sid_to_batch.get_mut(&sid) {
                batch.retain(|t| t != ticker);
                let channel_clone = channel.clone();
                let remaining = batch.clone();
                if batch.is_empty() {
                    self.sid_to_batch.remove(&sid);
                }
                result.push((sid, channel_clone, remaining));
            }
        }
        result
    }

}

/// Kalshi WebSocket client
pub struct KalshiWebSocket {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    command_id: u64,
    subscribed_markets: HashSet<String>,
    sid_tracker: SidTracker,
}

impl KalshiWebSocket {
    /// Connect to Kalshi WebSocket with authentication
    ///
    /// If `url` is provided, it overrides the default production/demo URL.
    pub async fn connect(
        credentials: &KalshiCredentials,
        use_demo: bool,
        url: Option<&str>,
    ) -> Result<Self, WebSocketError> {
        let url = url.unwrap_or(if use_demo {
            KALSHI_WS_DEMO_URL
        } else {
            KALSHI_WS_URL
        });

        let (timestamp, signature) = credentials.sign_websocket_request()?;

        let url_without_scheme = url.replace("wss://", "");
        let host = url_without_scheme
            .split('/')
            .next()
            .unwrap_or("api.kalshi.com");

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
            subscribed_markets: HashSet::new(),
            sid_tracker: SidTracker::new(),
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
                sids: None,
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
                sids: None,
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        debug!(cmd = %msg, market = %market_ticker, "Sending subscribe command");

        self.ws.send(Message::Text(msg)).await?;
        self.subscribed_markets.insert(market_ticker.to_string());

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
                sids: None,
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        debug!(cmd = %msg, "Subscribing to all trades");

        self.ws.send(Message::Text(msg)).await?;
        self.wait_for_subscription(self.command_id).await
    }

    /// Subscribe to market lifecycle events (all markets)
    ///
    /// This channel provides events when markets are created, activated, deactivated,
    /// have their close dates updated, are determined, or settled.
    pub async fn subscribe_lifecycle(&mut self) -> Result<(), WebSocketError> {
        self.command_id += 1;
        let cmd = WsCommand {
            id: self.command_id,
            cmd: "subscribe".to_string(),
            params: WsParams {
                channels: vec!["market_lifecycle_v2".to_string()],
                market_ticker: None,
                market_tickers: None,
                sids: None,
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        debug!(cmd = %msg, "Subscribing to market lifecycle events");

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
                sids: None,
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        self.ws.send(Message::Text(msg)).await?;

        self.wait_for_subscription(self.command_id).await
    }

    /// Subscribe to a channel for multiple markets (single subscription, max 256 markets)
    ///
    /// For more than MAX_MARKETS_PER_SUBSCRIPTION markets, use multiple WebSocket
    /// connections (sharding) at the connector level.
    pub async fn subscribe_markets(
        &mut self,
        channel: &str,
        tickers: &[String],
    ) -> Result<Option<u64>, WebSocketError> {
        if tickers.len() > MAX_MARKETS_PER_SUBSCRIPTION {
            return Err(WebSocketError::SubscriptionFailed(format!(
                "Too many markets ({}) for single subscription, max is {}. Use sharding.",
                tickers.len(),
                MAX_MARKETS_PER_SUBSCRIPTION
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
                sids: None,
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        info!(
            channel = %channel,
            markets = tickers.len(),
            "Subscribing to channel"
        );

        self.ws.send(Message::Text(msg)).await?;
        let sid = self.wait_for_subscription_with_sid(self.command_id).await?;

        self.subscribed_markets.extend(tickers.iter().cloned());

        if let Some(sid_val) = sid {
            self.sid_tracker.record_subscription(sid_val, channel, tickers);
        }

        info!(
            channel = %channel,
            markets = tickers.len(),
            ?sid,
            "Subscription confirmed"
        );

        Ok(sid)
    }

    /// Timeout for subscription confirmation
    const SUBSCRIPTION_TIMEOUT_SECS: u64 = 30;

    /// Wait for subscription confirmation
    async fn wait_for_subscription(&mut self, expected_id: u64) -> Result<(), WebSocketError> {
        self.wait_for_subscription_with_sid(expected_id).await?;
        Ok(())
    }

    /// Wait for subscription confirmation and return the subscription ID (sid)
    async fn wait_for_subscription_with_sid(
        &mut self,
        expected_id: u64,
    ) -> Result<Option<u64>, WebSocketError> {
        let timeout = tokio::time::timeout(Duration::from_secs(Self::SUBSCRIPTION_TIMEOUT_SECS), async {
            let mut message_count = 0u64;
            while let Some(msg) = self.ws.next().await {
                match msg? {
                    Message::Text(text) => {
                        message_count += 1;
                        match serde_json::from_str::<WsMessage>(&text) {
                            Ok(WsMessage::Subscribed { id, msg }) => {
                                if id == expected_id {
                                    let sid = msg.as_ref().map(|m| m.sid);
                                    let channel = msg.as_ref().map(|m| m.channel.as_str());
                                    info!(id, ?sid, ?channel, messages_received = message_count, "Subscription confirmed (subscribed)");
                                    return Ok(sid);
                                } else {
                                    debug!(id, expected = expected_id, "Received subscription confirmation for different id");
                                }
                            }
                            Ok(WsMessage::Ok { id, sid, seq, market_tickers }) => {
                                if id == expected_id {
                                    let ticker_count = market_tickers.as_ref().map(|t| t.len());
                                    info!(id, ?sid, ?seq, ?ticker_count, messages_received = message_count, "Subscription confirmed (ok)");
                                    return Ok(sid);
                                } else {
                                    debug!(id, expected = expected_id, "Received ok for different id");
                                }
                            }
                            Ok(WsMessage::Error { id, msg }) => {
                                let code = msg.as_ref().map(|m| m.code);
                                let error_msg = msg.as_ref().map(|m| m.msg.as_str());
                                warn!(
                                    ?id,
                                    ?code,
                                    ?error_msg,
                                    expected_id,
                                    "Received error from Kalshi"
                                );
                                if id == Some(expected_id) {
                                    return Err(WebSocketError::SubscriptionFailed(
                                        error_msg.map(|s| s.to_string()).unwrap_or_else(|| format!("Error code: {:?}", code))
                                    ));
                                }
                            }
                            Ok(WsMessage::Ticker { .. } | WsMessage::Trade { .. }) => {
                                // Expected during subscription - data is flowing
                                if message_count == 1 || message_count % 100 == 0 {
                                    debug!(message_count, expected_id, "Receiving data while waiting for subscription confirmation");
                                }
                            }
                            Ok(WsMessage::Unknown) => {
                                warn!(raw = %text, "Received unknown message type from Kalshi");
                            }
                            Ok(other) => {
                                debug!(?other, "Received non-data message");
                            }
                            Err(e) => {
                                warn!(error = %e, raw = %text, "Failed to parse WebSocket message");
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
            .map_err(|_| {
                warn!(expected_id, timeout_secs = Self::SUBSCRIPTION_TIMEOUT_SECS, "Subscription timeout - no confirmation received");
                WebSocketError::SubscriptionFailed("Timeout waiting for confirmation".into())
            })?
    }

    /// Read timeout in seconds - if no data received for this long, assume connection is dead
    const READ_TIMEOUT_SECS: u64 = 120;

    /// Receive the next message with raw text
    /// Returns (raw_json, parsed_message) for raw data capture
    ///
    /// Has a read timeout to detect silent connection deaths.
    /// If no data (including pings) is received within READ_TIMEOUT_SECS,
    /// returns an error so the connector can reconnect.
    pub async fn recv_raw(&mut self) -> Result<(String, WsMessage), WebSocketError> {
        loop {
            let recv_result = tokio::time::timeout(
                Duration::from_secs(Self::READ_TIMEOUT_SECS),
                self.ws.next(),
            )
            .await;

            match recv_result {
                Err(_) => {
                    // Timeout - no data received, connection likely dead
                    warn!(
                        timeout_secs = Self::READ_TIMEOUT_SECS,
                        "WebSocket read timeout - no data received, connection may be dead"
                    );
                    return Err(WebSocketError::Connection(format!(
                        "Read timeout after {} seconds - no data received",
                        Self::READ_TIMEOUT_SECS
                    )));
                }
                Ok(Some(Ok(Message::Text(text)))) => {
                    match serde_json::from_str::<WsMessage>(&text) {
                        Ok(msg) => {
                            trace!(msg = %text, "Received message");
                            return Ok((text, msg));
                        }
                        Err(e) => {
                            warn!(error = %e, text = %text, "Failed to parse message");
                            return Err(WebSocketError::ParseFailed {
                                error: e.to_string(),
                                raw_json: text,
                            });
                        }
                    }
                }
                Ok(Some(Ok(Message::Ping(data)))) => {
                    trace!("Received ping, sending pong");
                    self.ws.send(Message::Pong(data)).await?;
                }
                Ok(Some(Ok(Message::Close(frame)))) => {
                    info!(frame = ?frame, "WebSocket closed");
                    return Err(WebSocketError::ConnectionClosed);
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(e.into()),
                Ok(None) => return Err(WebSocketError::ConnectionClosed),
            }
        }
    }

    /// Send an unsubscribe command for the given subscription IDs.
    pub async fn unsubscribe_sids(&mut self, sids: &[u64]) -> Result<(), WebSocketError> {
        if sids.is_empty() {
            return Ok(());
        }

        self.command_id += 1;
        let cmd = WsCommand {
            id: self.command_id,
            cmd: "unsubscribe".to_string(),
            params: WsParams {
                channels: vec![],
                market_ticker: None,
                market_tickers: None,
                sids: Some(sids.to_vec()),
            },
        };

        let msg = serde_json::to_string(&cmd)?;
        info!(sids = ?sids, cmd_id = self.command_id, "Sending unsubscribe command");

        self.ws.send(Message::Text(msg)).await?;
        self.wait_for_unsubscribe(self.command_id).await
    }

    /// Wait for unsubscribe confirmation
    async fn wait_for_unsubscribe(&mut self, expected_id: u64) -> Result<(), WebSocketError> {
        let timeout = tokio::time::timeout(
            Duration::from_secs(Self::SUBSCRIPTION_TIMEOUT_SECS),
            async {
                while let Some(msg) = self.ws.next().await {
                    match msg? {
                        Message::Text(text) => {
                            match serde_json::from_str::<WsMessage>(&text) {
                                Ok(WsMessage::Unsubscribed { id }) if id == expected_id => {
                                    info!(id, "Unsubscribe confirmed");
                                    return Ok(());
                                }
                                Ok(WsMessage::Error { id, msg }) if id == Some(expected_id) => {
                                    let error_msg = msg.as_ref().map(|m| m.msg.as_str());
                                    return Err(WebSocketError::SubscriptionFailed(
                                        format!("Unsubscribe error: {}", error_msg.unwrap_or("unknown"))
                                    ));
                                }
                                _ => continue,
                            }
                        }
                        Message::Close(_) => return Err(WebSocketError::ConnectionClosed),
                        _ => continue,
                    }
                }
                Err(WebSocketError::ConnectionClosed)
            },
        );

        timeout
            .await
            .map_err(|_| WebSocketError::SubscriptionFailed("Unsubscribe timeout".into()))?
    }

    /// Unsubscribe a single market from all its channels.
    /// Handles batch semantics: unsubscribes the batch sid, then
    /// resubscribes remaining markets in each affected batch.
    /// Returns the number of sids unsubscribed.
    pub async fn unsubscribe_market(&mut self, ticker: &str) -> Result<usize, WebSocketError> {
        let affected = self.sid_tracker.remove_market(ticker);
        if affected.is_empty() {
            debug!(ticker, "Market not tracked in sid_tracker, nothing to unsubscribe");
            return Ok(0);
        }

        let mut unsubscribed_count = 0;

        for (old_sid, channel, remaining) in affected {
            // Unsubscribe the old batch
            self.unsubscribe_sids(&[old_sid]).await?;
            unsubscribed_count += 1;

            // Remove the settled market from our local set
            self.subscribed_markets.remove(ticker);

            // Resubscribe remaining markets in the batch (if any)
            if !remaining.is_empty() {
                match self.subscribe_markets(&channel, &remaining).await {
                    Ok(new_sid) => {
                        info!(
                            old_sid,
                            ?new_sid,
                            channel = %channel,
                            remaining = remaining.len(),
                            ticker,
                            "Resubscribed remaining markets after unsubscribe"
                        );
                    }
                    Err(e) => {
                        warn!(
                            old_sid,
                            channel = %channel,
                            remaining = remaining.len(),
                            ticker,
                            error = %e,
                            "Failed to resubscribe remaining markets after unsubscribe"
                        );
                        return Err(e);
                    }
                }
            } else {
                info!(old_sid, channel = %channel, ticker, "Batch empty after unsubscribe");
            }
        }

        Ok(unsubscribed_count)
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

    /// Get set of subscribed markets
    pub fn subscribed_markets(&self) -> &HashSet<String> {
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

    #[test]
    fn test_sid_tracker_record_and_lookup() {
        let mut tracker = SidTracker::new();
        tracker.record_subscription(42, "ticker", &["MARKET-A".into(), "MARKET-B".into()]);
        tracker.record_subscription(43, "trade", &["MARKET-A".into(), "MARKET-B".into()]);

        let sids = tracker.sids_for_ticker("MARKET-A");
        assert_eq!(sids.len(), 2);
        assert!(sids.contains(&42));
        assert!(sids.contains(&43));

        let (channel, tickers) = tracker.tickers_for_sid(42).unwrap();
        assert_eq!(channel, "ticker");
        assert_eq!(tickers.len(), 2);
        assert!(tickers.contains(&"MARKET-A".to_string()));
    }

    #[test]
    fn test_sid_tracker_remove_market() {
        let mut tracker = SidTracker::new();
        tracker.record_subscription(42, "ticker", &["A".into(), "B".into(), "C".into()]);

        let result = tracker.remove_market("B");
        assert_eq!(result.len(), 1);
        let (sid, channel, remaining) = &result[0];
        assert_eq!(*sid, 42);
        assert_eq!(channel, "ticker");
        assert_eq!(remaining.len(), 2);
        assert!(remaining.contains(&"A".to_string()));
        assert!(remaining.contains(&"C".to_string()));
        assert!(!remaining.contains(&"B".to_string()));

        assert!(tracker.sids_for_ticker("B").is_empty());
    }

    #[test]
    fn test_sid_tracker_remove_last_in_batch() {
        let mut tracker = SidTracker::new();
        tracker.record_subscription(42, "ticker", &["ONLY".into()]);

        let result = tracker.remove_market("ONLY");
        assert_eq!(result.len(), 1);
        let (_, _, remaining) = &result[0];
        assert!(remaining.is_empty());
        assert!(tracker.tickers_for_sid(42).is_none());
    }

    #[test]
    fn test_sid_tracker_remove_nonexistent() {
        let mut tracker = SidTracker::new();
        let result = tracker.remove_market("DOESNT-EXIST");
        assert!(result.is_empty());
    }

    #[test]
    fn test_ws_unsubscribe_command_serialization() {
        let cmd = WsCommand {
            id: 10,
            cmd: "unsubscribe".to_string(),
            params: WsParams {
                channels: vec![],
                market_ticker: None,
                market_tickers: None,
                sids: Some(vec![42, 43]),
            },
        };

        let json = serde_json::to_string(&cmd).expect("Failed to serialize");
        assert!(json.contains(r#""cmd":"unsubscribe""#));
        assert!(json.contains(r#""sids":[42,43]"#));
    }
}
