//! Kraken Futures connector — Connector trait implementation.
//!
//! Spawns a receiver task that:
//! 1. Connects to wss://futures.kraken.com/ws/v1
//! 2. Subscribes to "trade" and "ticker" feeds for configured product IDs
//! 3. Sends pings every 60s
//! 4. Forwards data messages to the MPSC channel
//! 5. Tracks last activity time for health checks

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, trace, warn};

use super::messages::KrakenFuturesWsMessage;
use super::websocket::{KrakenFuturesWebSocket, KrakenFuturesWsError, PING_INTERVAL_SECS};
use crate::error::ConnectorError;
use crate::metrics::{ConnectorMetrics, ShardMetrics};
use crate::traits::Connector;

pub struct KrakenFuturesConnector {
    product_ids: Vec<String>,
    /// WebSocket URL override from feed config (None = use default constant)
    ws_url: Option<String>,
    tx: Option<mpsc::Sender<Vec<u8>>>,
    rx: Option<mpsc::Receiver<Vec<u8>>>,
    last_ws_activity_epoch_secs: Arc<AtomicU64>,
}

impl KrakenFuturesConnector {
    pub fn new(product_ids: Vec<String>, ws_url: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel(4096);
        Self {
            product_ids,
            ws_url,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    fn spawn_receiver_task(
        mut ws: KrakenFuturesWebSocket,
        _product_ids: Vec<String>,
        tx: mpsc::Sender<Vec<u8>>,
        last_activity: Arc<AtomicU64>,
        shard_metrics: ShardMetrics,
    ) {
        fn update_activity(tracker: &AtomicU64, metrics: &ShardMetrics, idle_secs: f64) {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            tracker.store(now, Ordering::SeqCst);
            metrics.set_last_activity(now as f64);
            metrics.set_idle_seconds(idle_secs);
        }

        // Mark shard as connected
        shard_metrics.set_connected();

        // Initialize activity tracker
        update_activity(&last_activity, &shard_metrics, 0.0);

        tokio::spawn(async move {
            use std::time::Duration;
            use tokio::time::{interval, Instant};

            let connected_at = Instant::now();
            let mut message_count: u64 = 0;

            let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
            ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            let mut last_activity_instant = Instant::now();

            loop {
                tokio::select! {
                    // Ping timer
                    _ = ping_interval.tick() => {
                        let idle_secs = last_activity_instant.elapsed().as_secs();
                        trace!(idle_secs, "Sending Kraken Futures ping");
                        shard_metrics.set_idle_seconds(idle_secs as f64);
                        if let Err(e) = ws.ping().await {
                            let uptime_secs = connected_at.elapsed().as_secs();
                            error!(
                                error = %e,
                                uptime_secs,
                                message_count,
                                reason = "ping_failed",
                                "Kraken Futures ping failed, exiting for restart"
                            );
                            shard_metrics.set_disconnected();
                            std::process::exit(1);
                        }
                        update_activity(&last_activity, &shard_metrics, idle_secs as f64);
                    }

                    // Receive message from WebSocket
                    result = ws.recv_raw() => {
                        last_activity_instant = Instant::now();
                        update_activity(&last_activity, &shard_metrics, 0.0);

                        match result {
                            Ok((raw, msg)) => {
                                message_count += 1;
                                match &msg {
                                    KrakenFuturesWsMessage::DataMessage { feed, product_id, .. } => {
                                        trace!(feed = %feed, product_id = %product_id, "Kraken Futures data message");
                                        match feed.as_str() {
                                            "ticker" | "ticker_lite" => shard_metrics.inc_ticker(),
                                            "trade" | "trade_snapshot" => shard_metrics.inc_trade(),
                                            "book" | "book_snapshot" => shard_metrics.inc_orderbook(),
                                            other => shard_metrics.inc_message(other),
                                        }
                                        if tx.send(raw.into_bytes()).await.is_err() {
                                            warn!("Channel closed, exiting receiver");
                                            shard_metrics.set_disconnected();
                                            return;
                                        }
                                    }
                                    KrakenFuturesWsMessage::Heartbeat { .. } => {
                                        shard_metrics.inc_message("heartbeat");
                                        trace!("Kraken Futures heartbeat received");
                                    }
                                    KrakenFuturesWsMessage::Error { message, .. } => {
                                        shard_metrics.inc_message("error");
                                        warn!(message = ?message, "Kraken Futures WS error");
                                    }
                                    KrakenFuturesWsMessage::Info { .. } => {
                                        shard_metrics.inc_message("info");
                                    }
                                    KrakenFuturesWsMessage::Subscribed { .. } => {
                                        shard_metrics.inc_message("subscribed");
                                    }
                                }
                            }
                            Err(e) => {
                                let uptime_secs = connected_at.elapsed().as_secs();
                                let reason = match &e {
                                    KrakenFuturesWsError::ConnectionClosed => "connection_closed",
                                    KrakenFuturesWsError::ReadTimeout => "read_timeout",
                                    _ => "ws_error",
                                };
                                error!(
                                    error = %e,
                                    uptime_secs,
                                    message_count,
                                    reason,
                                    "Kraken Futures WebSocket disconnect, exiting for restart"
                                );
                                shard_metrics.set_disconnected();
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }

        });
    }
}

#[async_trait]
impl Connector for KrakenFuturesConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        let tx = self.tx.take().ok_or_else(|| {
            ConnectorError::ConnectionFailed("Already connected".to_string())
        })?;

        // Initialize Prometheus metrics
        let connector_metrics = ConnectorMetrics::new("kraken-futures", "perpetuals");
        connector_metrics.set_shards_total(1);
        connector_metrics.set_markets_subscribed(0, self.product_ids.len());
        let shard_metrics = connector_metrics.for_shard(0);

        // Connect to Kraken Futures WS
        let mut ws = KrakenFuturesWebSocket::connect(self.ws_url.as_deref())
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        // Subscribe to trade and ticker feeds
        for feed in &["trade", "ticker"] {
            let result: Result<(), KrakenFuturesWsError> = ws.subscribe(feed, &self.product_ids).await;
            result.map_err(|e| ConnectorError::ConnectionFailed(format!("{} subscription: {}", feed, e)))?;
        }

        info!(products = ?self.product_ids, "Kraken Futures connector started");

        Self::spawn_receiver_task(
            ws,
            self.product_ids.clone(),
            tx,
            Arc::clone(&self.last_ws_activity_epoch_secs),
            shard_metrics,
        );

        Ok(())
    }

    fn messages(&mut self) -> mpsc::Receiver<Vec<u8>> {
        self.rx
            .take()
            .expect("messages() called before connect() or called twice")
    }

    async fn close(&mut self) -> Result<(), ConnectorError> {
        self.tx = None;
        info!("Kraken Futures connector closing");
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
            KrakenFuturesConnector::new(vec!["PF_XBTUSD".to_string(), "PF_ETHUSD".to_string()], None);
        assert!(connector.tx.is_some());
        assert!(connector.rx.is_some());
        assert_eq!(connector.product_ids.len(), 2);
    }

    #[test]
    fn test_connector_messages_takes_receiver() {
        let mut connector = KrakenFuturesConnector::new(vec!["PF_XBTUSD".to_string()], None);
        let _rx = connector.messages();
        assert!(connector.rx.is_none());
    }

    #[test]
    fn test_connector_activity_handle() {
        let connector = KrakenFuturesConnector::new(vec!["PF_XBTUSD".to_string()], None);
        let handle = connector.activity_handle();
        assert!(handle.is_some());
    }
}
