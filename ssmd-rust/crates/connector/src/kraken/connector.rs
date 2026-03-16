//! Kraken connector implementation
//!
//! Implements the ssmd Connector trait for Kraken v2 WebSocket.
//! Much simpler than Kalshi - no auth, no sharding, no CDC.

use crate::error::ConnectorError;
use crate::kraken::messages::KrakenWsMessage;
use crate::kraken::websocket::{KrakenWebSocket, KrakenWebSocketError};
use crate::metrics::ConnectorMetrics;
use crate::traits::{Connector, TimestampedMsg};
use async_trait::async_trait;
use ssmd_middleware::now_tsc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

/// Kraken connector implementing the ssmd Connector trait
pub struct KrakenConnector {
    symbols: Vec<String>,
    /// Feed name for metrics labels (e.g., "kraken-spot", "kraken")
    feed_name: String,
    /// WebSocket URL override from feed config (None = use default constant)
    ws_url: Option<String>,
    tx: Option<mpsc::Sender<TimestampedMsg>>,
    rx: Option<mpsc::Receiver<TimestampedMsg>>,
    /// Last WebSocket activity timestamp (epoch seconds)
    last_ws_activity_epoch_secs: Arc<AtomicU64>,
}

impl KrakenConnector {
    /// Create a new Kraken connector with the given symbols
    pub fn new(symbols: Vec<String>, ws_url: Option<String>) -> Self {
        Self::with_feed_name(symbols, ws_url, "kraken".to_string())
    }

    /// Create a new Kraken connector with an explicit feed name for metrics
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

    /// Spawn the WebSocket receiver task
    fn spawn_receiver_task(
        mut ws: KrakenWebSocket,
        tx: mpsc::Sender<TimestampedMsg>,
        activity_tracker: Arc<AtomicU64>,
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
                    // Ping timer - send app-level ping
                    _ = ping_interval.tick() => {
                        trace!("Sending Kraken app-level ping");
                        if let Err(e) = ws.ping().await {
                            error!(error = %e, "Failed to send Kraken ping, connection may be dead");
                            break;
                        }
                        update_activity(&activity_tracker);
                    }

                    // Receive message from WebSocket
                    result = ws.recv_raw() => {
                        update_activity(&activity_tracker);

                        match result {
                            Ok((raw_json, msg)) => {
                                let should_forward = matches!(
                                    &msg,
                                    KrakenWsMessage::ChannelMessage { channel, .. }
                                    if channel == "ticker" || channel == "trade"
                                );

                                if !should_forward {
                                    match &msg {
                                        KrakenWsMessage::Heartbeat { .. } => {
                                            trace!("Kraken heartbeat received");
                                        }
                                        KrakenWsMessage::Pong { .. } => {
                                            trace!("Kraken pong received");
                                        }
                                        KrakenWsMessage::SubscriptionResult { .. } => {
                                            debug!("Kraken subscription result received");
                                        }
                                        _ => {}
                                    }
                                    continue;
                                }

                                // Forward raw JSON bytes
                                if tx.send((now_tsc(), raw_json.into_bytes())).await.is_err() {
                                    info!("Channel closed, stopping Kraken receiver");
                                    break;
                                }
                            }
                            Err(KrakenWebSocketError::ConnectionClosed) => {
                                error!("Kraken WebSocket connection closed, exiting for restart");
                                std::process::exit(1);
                            }
                            Err(e) => {
                                error!(error = %e, "Kraken WebSocket error, exiting for restart");
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }

            // Try to close gracefully
            if let Err(e) = ws.close().await {
                error!(error = %e, "Error closing Kraken WebSocket");
            }
        });
    }
}

#[async_trait]
impl Connector for KrakenConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        let tx = self.tx.take().ok_or_else(|| {
            ConnectorError::ConnectionFailed("connect() called twice".to_string())
        })?;

        let activity_tracker = Arc::clone(&self.last_ws_activity_epoch_secs);

        // Create metrics — use feed_name so kraken-spot emits feed="kraken-spot"
        let connector_metrics = ConnectorMetrics::new(&self.feed_name, "spot");
        connector_metrics.set_shards_total(1);
        // Pre-init MESSAGES_TOTAL so the feed label exists in Prometheus
        connector_metrics.for_shard(0).init(&["ticker", "trade"]);

        // Connect to Kraken WS
        let mut ws = KrakenWebSocket::connect(self.ws_url.as_deref())
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        // Subscribe to ticker channel (Kraken sends one result per symbol)
        info!(symbols = ?self.symbols, count = self.symbols.len(), "Subscribing to Kraken ticker channel");
        let ticker_symbols = ws.subscribe("ticker", &self.symbols)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("ticker subscription: {}", e)))?;

        // Subscribe to trade channel
        info!(symbols = ?self.symbols, count = self.symbols.len(), "Subscribing to Kraken trade channel");
        let trade_symbols = ws.subscribe("trade", &self.symbols)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("trade subscription: {}", e)))?;

        // Update metrics with actual subscribed count (use ticker as the canonical count)
        connector_metrics.set_markets_subscribed(0, ticker_symbols.len());

        info!(
            ticker_subscribed = ticker_symbols.len(),
            trade_subscribed = trade_symbols.len(),
            requested = self.symbols.len(),
            "Kraken connector subscribed to ticker and trade channels"
        );

        // Spawn receiver task
        Self::spawn_receiver_task(ws, tx, activity_tracker);

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
        let connector = KrakenConnector::new(vec!["BTC/USD".to_string(), "ETH/USD".to_string()], None);
        assert!(connector.tx.is_some());
        assert!(connector.rx.is_some());
        assert_eq!(connector.symbols.len(), 2);
    }

    #[test]
    fn test_connector_messages_takes_receiver() {
        let mut connector =
            KrakenConnector::new(vec!["BTC/USD".to_string()], None);
        let _rx = connector.messages();
        assert!(connector.rx.is_none());
    }

    #[test]
    fn test_connector_activity_handle() {
        let connector = KrakenConnector::new(vec!["BTC/USD".to_string()], None);
        let handle = connector.activity_handle();
        assert!(handle.is_some());
    }
}
