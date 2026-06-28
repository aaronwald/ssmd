//! Binance connector implementation
//!
//! Implements the ssmd `Connector` trait for the Binance spot combined-stream
//! WebSocket. Like Kraken spot: no auth, no sharding, no CDC. Forwards raw
//! `@trade` frames verbatim to the writer, which routes them to NATS.

use crate::binance::messages::BinanceWsMessage;
use crate::binance::websocket::{BinanceWebSocket, BinanceWebSocketError};
use crate::error::ConnectorError;
use crate::metrics::ConnectorMetrics;
use crate::traits::{Connector, TimestampedMsg};
use async_trait::async_trait;
use ssmd_middleware::now_tsc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

/// Binance connector implementing the ssmd `Connector` trait.
pub struct BinanceConnector {
    symbols: Vec<String>,
    /// Feed name for metrics labels (e.g., "binance").
    feed_name: String,
    /// WebSocket base URL override from feed config (None = use default constant).
    ws_url: Option<String>,
    tx: Option<mpsc::Sender<TimestampedMsg>>,
    rx: Option<mpsc::Receiver<TimestampedMsg>>,
    /// Last WebSocket activity timestamp (epoch seconds).
    last_ws_activity_epoch_secs: Arc<AtomicU64>,
}

impl BinanceConnector {
    /// Create a new Binance connector with the given symbols.
    pub fn new(symbols: Vec<String>, ws_url: Option<String>) -> Self {
        Self::with_feed_name(symbols, ws_url, "binance".to_string())
    }

    /// Create a new Binance connector with an explicit feed name for metrics.
    pub fn with_feed_name(symbols: Vec<String>, ws_url: Option<String>, feed_name: String) -> Self {
        let (tx, rx) = mpsc::channel(1000);
        Self {
            symbols,
            feed_name,
            ws_url,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Spawn the WebSocket receiver task.
    fn spawn_receiver_task(
        mut ws: BinanceWebSocket,
        tx: mpsc::Sender<TimestampedMsg>,
        activity_tracker: Arc<AtomicU64>,
        shard_metrics: crate::metrics::ShardMetrics,
    ) {
        fn update_activity(tracker: &AtomicU64) {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            tracker.store(now, Ordering::SeqCst);
        }

        // Initialize activity tracker
        update_activity(&activity_tracker);

        tokio::spawn(async move {
            use std::time::Duration;
            use tokio::time::interval;

            const PING_INTERVAL_SECS: u64 = 30;

            let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
            ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    // Ping timer — send WS-level keepalive ping
                    _ = ping_interval.tick() => {
                        trace!("Sending Binance WS ping");
                        if let Err(e) = ws.ping().await {
                            error!(error = %e, "Failed to send Binance ping, connection may be dead");
                            break;
                        }
                        update_activity(&activity_tracker);
                    }

                    // Receive message from WebSocket
                    result = ws.recv_raw() => {
                        match result {
                            Ok((raw_json, msg)) => {
                                // Only update activity on successful data — not errors
                                update_activity(&activity_tracker);

                                let should_forward = match &msg {
                                    BinanceWsMessage::Combined { data, .. }
                                        if data.event_type == "trade" =>
                                    {
                                        shard_metrics.inc_trade();
                                        true
                                    }
                                    BinanceWsMessage::Combined { data, .. } => {
                                        trace!(event = %data.event_type, "Skipping non-trade Binance frame");
                                        false
                                    }
                                    BinanceWsMessage::CommandResult { .. } => {
                                        debug!("Binance command result received");
                                        false
                                    }
                                    BinanceWsMessage::Error { error } => {
                                        error!(error = ?error, "Binance error frame received");
                                        false
                                    }
                                };

                                if !should_forward {
                                    continue;
                                }

                                // Forward raw JSON bytes — the whole combined-stream frame.
                                // `raw_json` originates from a tungstenite Text frame, which is
                                // guaranteed valid UTF-8.
                                if tx.send((now_tsc(), raw_json.into_bytes())).await.is_err() {
                                    info!("Channel closed, stopping Binance receiver");
                                    break;
                                }
                            }
                            Err(BinanceWebSocketError::ConnectionClosed) => {
                                error!("Binance WebSocket connection closed, exiting for restart");
                                std::process::exit(1);
                            }
                            Err(e) => {
                                error!(error = %e, "Binance WebSocket error, exiting for restart");
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }

            // Try to close gracefully
            if let Err(e) = ws.close().await {
                error!(error = %e, "Error closing Binance WebSocket");
            }
        });
    }
}

#[async_trait]
impl Connector for BinanceConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        // Fail loud at the connector boundary: refuse to start with no
        // subscriptions. An empty symbol list is an unrecoverable
        // misconfiguration, not something to limp along with.
        let non_empty: Vec<&String> = self
            .symbols
            .iter()
            .filter(|s| !s.trim().is_empty())
            .collect();
        if non_empty.is_empty() {
            return Err(ConnectorError::ConnectionFailed(
                "Binance connector requires at least one non-empty symbol".to_string(),
            ));
        }

        let tx = self.tx.take().ok_or_else(|| {
            ConnectorError::ConnectionFailed("connect() called twice".to_string())
        })?;

        let activity_tracker = Arc::clone(&self.last_ws_activity_epoch_secs);

        // Create metrics — use feed_name so binance emits feed="binance"
        let connector_metrics = ConnectorMetrics::new(&self.feed_name, "spot");
        connector_metrics.set_shards_total(1);
        // Pre-init MESSAGES_TOTAL so the feed label exists in Prometheus
        connector_metrics.for_shard(0).init(&["trade"]);

        // Connect to the Binance combined stream (streams encoded in the URL).
        info!(
            symbols = ?self.symbols,
            count = self.symbols.len(),
            "Subscribing to Binance @trade combined stream"
        );
        let mut ws = BinanceWebSocket::connect(self.ws_url.as_deref(), &self.symbols)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        // No subscribe handshake — combined-stream URL begins delivering trades
        // immediately. Probe the link with one keepalive ping so a dead socket
        // fails fast at connect rather than after the first read timeout.
        ws.ping()
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("initial ping: {}", e)))?;

        connector_metrics.set_markets_subscribed(0, non_empty.len());

        info!(
            subscribed = non_empty.len(),
            "Binance connector subscribed to @trade combined stream"
        );

        // Spawn receiver task with shard metrics for message counting
        let shard_metrics = connector_metrics.for_shard(0);
        Self::spawn_receiver_task(ws, tx, activity_tracker, shard_metrics);

        Ok(())
    }

    fn messages(&mut self) -> mpsc::Receiver<TimestampedMsg> {
        self.rx
            .take()
            .expect("messages() called before connect() or called twice")
    }

    async fn close(&mut self) -> Result<(), ConnectorError> {
        // Drop the sender to signal the spawned task to stop
        self.tx = None;
        Ok(())
    }

    fn activity_handle(&self) -> Option<Arc<AtomicU64>> {
        Some(Arc::clone(&self.last_ws_activity_epoch_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_creation() {
        let connector =
            BinanceConnector::new(vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()], None);
        assert!(connector.tx.is_some());
        assert!(connector.rx.is_some());
        assert_eq!(connector.symbols.len(), 2);
        assert_eq!(connector.feed_name, "binance");
    }

    #[test]
    fn test_connector_with_feed_name() {
        let connector = BinanceConnector::with_feed_name(
            vec!["BTCUSDT".to_string()],
            Some("wss://example.test".to_string()),
            "binance".to_string(),
        );
        assert_eq!(connector.feed_name, "binance");
        assert_eq!(connector.ws_url.as_deref(), Some("wss://example.test"));
    }

    #[test]
    fn test_connector_messages_takes_receiver() {
        let mut connector = BinanceConnector::new(vec!["BTCUSDT".to_string()], None);
        let _rx = connector.messages();
        assert!(connector.rx.is_none());
    }

    #[test]
    fn test_connector_activity_handle() {
        let connector = BinanceConnector::new(vec!["BTCUSDT".to_string()], None);
        let handle = connector.activity_handle();
        assert!(handle.is_some());
    }

    #[tokio::test]
    async fn connect_rejects_empty_symbol_list() {
        let mut connector = BinanceConnector::new(vec!["   ".to_string()], None);
        let result = connector.connect().await;
        assert!(result.is_err());
    }
}
