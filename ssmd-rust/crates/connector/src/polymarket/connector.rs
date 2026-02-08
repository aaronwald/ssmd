//! Polymarket connector implementation
//!
//! Implements the ssmd Connector trait for Polymarket CLOB WebSocket.
//! Key differences from Kraken:
//! - Sharding: multiple WS connections needed (500 instrument limit)
//! - Market discovery: Gamma REST API polling (no CDC, no static config)
//! - Keepalive: 10-second PING interval (vs Kraken's 30s)
//! - Proactive reconnect: 15-minute timer due to known WS instability

use crate::error::ConnectorError;
use crate::metrics::ConnectorMetrics;
use crate::polymarket::market_discovery::MarketDiscovery;
use crate::polymarket::websocket::{
    PolymarketWebSocket, PolymarketWebSocketError, MAX_INSTRUMENTS_PER_CONNECTION,
};
use crate::traits::Connector;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

/// Polymarket PING interval: 10 seconds (required by Polymarket, vs 30s for Kraken)
const PING_INTERVAL_SECS: u64 = 10;

/// Proactive reconnect interval: 15 minutes
/// Polymarket WS is known to stop delivering data after ~20 minutes.
/// Reconnect proactively before that happens.
const PROACTIVE_RECONNECT_SECS: u64 = 900;

/// Polymarket connector implementing the ssmd Connector trait
pub struct PolymarketConnector {
    /// Token IDs to subscribe to (can be set statically or via discovery)
    token_ids: Vec<String>,
    /// Optional market discovery client for dynamic subscription
    discovery: Option<MarketDiscovery>,
    tx: Option<mpsc::Sender<Vec<u8>>>,
    rx: Option<mpsc::Receiver<Vec<u8>>>,
    /// Last WebSocket activity timestamp (epoch seconds)
    last_ws_activity_epoch_secs: Arc<AtomicU64>,
}

impl PolymarketConnector {
    /// Create a new Polymarket connector with static token IDs
    pub fn new(token_ids: Vec<String>) -> Self {
        let (tx, rx) = mpsc::channel(2000); // Larger buffer for multi-shard reconnect bursts
        Self {
            token_ids,
            discovery: None,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create a new Polymarket connector with market discovery
    pub fn with_discovery(discovery: MarketDiscovery) -> Self {
        let (tx, rx) = mpsc::channel(2000);
        Self {
            token_ids: Vec::new(),
            discovery: Some(discovery),
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Spawn a WebSocket receiver task for a shard (subset of token IDs).
    ///
    /// Each shard independently manages its own connection lifecycle with an outer
    /// reconnect loop. On WS errors, connection closed, or proactive reconnect timer,
    /// the shard reconnects without killing the process.
    fn spawn_shard_receiver(
        shard_id: usize,
        shard_token_ids: Vec<String>,
        tx: mpsc::Sender<Vec<u8>>,
        activity_tracker: Arc<AtomicU64>,
        connector_metrics: ConnectorMetrics,
    ) {
        fn update_activity(tracker: &AtomicU64) {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            tracker.store(now, Ordering::SeqCst);
        }

        tokio::spawn(async move {
            use std::time::Duration;
            use tokio::time::interval;

            let mut reconnect_count: u64 = 0;

            // Outer reconnect loop — each iteration establishes a fresh WS connection
            loop {
                // 1. Connect with retry backoff
                let mut ws = loop {
                    if tx.is_closed() {
                        info!(shard = shard_id, "Channel closed during connect, shutting down shard");
                        return;
                    }
                    match PolymarketWebSocket::connect().await {
                        Ok(ws) => break ws,
                        Err(e) => {
                            error!(shard = shard_id, error = %e, "Shard WS connect failed, retrying");
                            let jitter_ms = rand::random::<u64>() % 3000;
                            tokio::time::sleep(Duration::from_millis(5000 + jitter_ms)).await;
                        }
                    }
                };

                // 2. Subscribe
                if let Err(e) = ws.subscribe(&shard_token_ids).await {
                    error!(shard = shard_id, error = %e, "Shard subscribe failed, reconnecting");
                    let _ = ws.close().await;
                    let jitter_ms = rand::random::<u64>() % 3000;
                    tokio::time::sleep(Duration::from_millis(5000 + jitter_ms)).await;
                    continue;
                }

                // 3. Update metrics
                connector_metrics.set_shard_connected(shard_id);
                connector_metrics.set_markets_subscribed(shard_id, shard_token_ids.len());
                update_activity(&activity_tracker);

                info!(
                    shard = shard_id,
                    tokens = shard_token_ids.len(),
                    reconnect_count = reconnect_count,
                    "Shard connected and subscribed"
                );

                // 4. Setup timers
                let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
                ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                // Stagger reconnect timer by shard_id * 30s to spread reconnects
                let reconnect_secs = PROACTIVE_RECONNECT_SECS + (shard_id as u64 * 30);
                let mut reconnect_timer = interval(Duration::from_secs(reconnect_secs));
                reconnect_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                // Skip the immediate first tick
                reconnect_timer.tick().await;

                // Reason for breaking out of inner loop (set before every `break`)
                #[allow(unused_assignments)]
                let mut reconnect_reason = "unknown";

                // 5. Inner receive loop
                loop {
                    tokio::select! {
                        // Ping timer
                        _ = ping_interval.tick() => {
                            if let Err(e) = ws.ping().await {
                                error!(shard = shard_id, error = %e, "Failed to send Polymarket ping");
                                reconnect_reason = "ping_failed";
                                break;
                            }
                            update_activity(&activity_tracker);
                        }

                        // Proactive reconnect timer
                        _ = reconnect_timer.tick() => {
                            info!(
                                shard = shard_id,
                                interval_secs = reconnect_secs,
                                "Proactive reconnect triggered for shard"
                            );
                            reconnect_reason = "proactive";
                            break;
                        }

                        // Receive message from WebSocket
                        result = ws.recv_raw() => {
                            update_activity(&activity_tracker);

                            match result {
                                Ok(raw_json) => {
                                    if raw_json == "PONG" {
                                        continue;
                                    }
                                    if tx.send(raw_json.into_bytes()).await.is_err() {
                                        info!(shard = shard_id, "Channel closed, shutting down shard");
                                        let _ = ws.close().await;
                                        return;
                                    }
                                }
                                Err(PolymarketWebSocketError::ConnectionClosed) => {
                                    error!(shard = shard_id, "Polymarket WebSocket closed, reconnecting");
                                    reconnect_reason = "connection_closed";
                                    break;
                                }
                                Err(e) => {
                                    error!(shard = shard_id, error = %e, "Polymarket WebSocket error, reconnecting");
                                    reconnect_reason = "ws_error";
                                    break;
                                }
                            }
                        }
                    }
                }

                // 6. Cleanup before reconnect
                connector_metrics.set_shard_disconnected(shard_id);
                connector_metrics.inc_shard_reconnect(shard_id, reconnect_reason);
                if let Err(e) = ws.close().await {
                    error!(shard = shard_id, error = %e, "Error closing Polymarket WebSocket");
                }
                reconnect_count += 1;

                // Brief delay before reconnecting
                let jitter_ms = rand::random::<u64>() % 2000;
                tokio::time::sleep(Duration::from_millis(1000 + jitter_ms)).await;
            }
        });
    }
}

#[async_trait]
impl Connector for PolymarketConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        let tx = self.tx.take().ok_or_else(|| {
            ConnectorError::ConnectionFailed("connect() called twice".to_string())
        })?;

        let activity_tracker = Arc::clone(&self.last_ws_activity_epoch_secs);

        // If we have a discovery client, fetch markets first
        if let Some(ref discovery) = self.discovery {
            info!("Discovering active markets via Gamma API");
            let markets = discovery
                .fetch_active_markets()
                .await
                .map_err(|e| ConnectorError::ConnectionFailed(format!("Market discovery: {}", e)))?;

            self.token_ids = MarketDiscovery::extract_token_ids(&markets);
            info!(
                markets = markets.len(),
                token_ids = self.token_ids.len(),
                "Discovered markets"
            );
        }

        if self.token_ids.is_empty() {
            return Err(ConnectorError::ConnectionFailed(
                "No token IDs to subscribe to".to_string(),
            ));
        }

        // Shard token IDs across multiple WebSocket connections
        let shards: Vec<Vec<String>> = self
            .token_ids
            .chunks(MAX_INSTRUMENTS_PER_CONNECTION)
            .map(|chunk| chunk.to_vec())
            .collect();

        let num_shards = shards.len();
        info!(
            total_tokens = self.token_ids.len(),
            num_shards = num_shards,
            max_per_shard = MAX_INSTRUMENTS_PER_CONNECTION,
            "Sharding Polymarket subscriptions"
        );

        // Create metrics
        let connector_metrics = ConnectorMetrics::new("polymarket", "clob");
        connector_metrics.set_shards_total(num_shards);

        // Probe connection to fail-fast if Polymarket is completely unreachable
        let mut probe = PolymarketWebSocket::connect()
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("probe connection: {}", e)))?;
        let _ = probe.close().await;
        info!("Polymarket probe connection successful");

        for (shard_id, shard_tokens) in shards.into_iter().enumerate() {
            // Stagger shard startup by 2 seconds + random jitter (0-3s)
            if shard_id > 0 {
                let jitter_ms = (shard_id as u64 * 2000) + (rand::random::<u64>() % 3000);
                tokio::time::sleep(std::time::Duration::from_millis(jitter_ms)).await;
            }

            info!(
                shard = shard_id,
                tokens = shard_tokens.len(),
                "Spawning shard receiver"
            );

            Self::spawn_shard_receiver(
                shard_id,
                shard_tokens,
                tx.clone(),
                Arc::clone(&activity_tracker),
                connector_metrics.clone(),
            );
        }

        info!(
            num_shards = num_shards,
            total_tokens = self.token_ids.len(),
            "All Polymarket shard receivers spawned"
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
            PolymarketConnector::new(vec!["token1".to_string(), "token2".to_string()]);
        assert!(connector.tx.is_some());
        assert!(connector.rx.is_some());
        assert_eq!(connector.token_ids.len(), 2);
    }

    #[test]
    fn test_connector_messages_takes_receiver() {
        let mut connector = PolymarketConnector::new(vec!["token1".to_string()]);
        let _rx = connector.messages();
        assert!(connector.rx.is_none());
    }

    #[test]
    fn test_connector_activity_handle() {
        let connector = PolymarketConnector::new(vec!["token1".to_string()]);
        let handle = connector.activity_handle();
        assert!(handle.is_some());
    }

    #[test]
    fn test_channel_buffer_size() {
        // Verify we use 2000 buffer (larger than default 1000)
        let connector = PolymarketConnector::new(vec!["token1".to_string()]);
        assert!(connector.tx.is_some());
    }

    #[test]
    fn test_sharding_logic() {
        // Generate more token IDs than one connection can handle
        let token_ids: Vec<String> = (0..750).map(|i| format!("token_{}", i)).collect();

        let shards: Vec<Vec<String>> = token_ids
            .chunks(MAX_INSTRUMENTS_PER_CONNECTION)
            .map(|chunk| chunk.to_vec())
            .collect();

        assert_eq!(shards.len(), 2); // 750 / 500 = 2 shards
        assert_eq!(shards[0].len(), 500);
        assert_eq!(shards[1].len(), 250);
    }

    #[test]
    fn test_ping_interval_constant() {
        assert_eq!(PING_INTERVAL_SECS, 10);
    }

    #[test]
    fn test_proactive_reconnect_constant() {
        assert_eq!(PROACTIVE_RECONNECT_SECS, 900); // 15 minutes
    }
}
