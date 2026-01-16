//! Kalshi connector implementation
//!
//! Implements the ssmd Connector trait for Kalshi WebSocket.
//!
//! ## Subscription Modes
//!
//! - **Global mode**: Subscribes to all markets (original behavior)
//! - **Filtered mode**: Subscribes only to markets in configured categories
//!
//! ## Sharding
//!
//! Kalshi limits subscriptions to 256 markets per WebSocket. For categories with more
//! markets, the connector automatically creates multiple WebSocket connections (shards),
//! each handling up to 256 markets. Messages from all shards are merged into a single
//! output channel.
//!
//! ## TODO: CDC Dynamic Updates
//!
//! Currently, filtered subscriptions are static - markets are fetched once at startup.
//! Future enhancement: Subscribe to CDC stream from secmaster to dynamically add/remove
//! market subscriptions as markets are added/removed from categories.
//!
//! See: <https://github.com/aaronwald/ssmd/issues/TBD>

use crate::error::ConnectorError;
use crate::kalshi::auth::KalshiCredentials;
use crate::kalshi::websocket::{KalshiWebSocket, WebSocketError, MAX_MARKETS_PER_SUBSCRIPTION};
use crate::kalshi::messages::WsMessage;
use crate::metrics::{ConnectorMetrics, ShardMetrics};
use crate::secmaster::SecmasterClient;
use crate::traits::Connector;
use async_trait::async_trait;
use ssmd_metadata::{SecmasterConfig, SubscriptionConfig};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Kalshi connector implementing the ssmd Connector trait
pub struct KalshiConnector {
    credentials: KalshiCredentials,
    use_demo: bool,
    secmaster_config: Option<SecmasterConfig>,
    subscription_config: SubscriptionConfig,
    tx: Option<mpsc::Sender<Vec<u8>>>,
    rx: Option<mpsc::Receiver<Vec<u8>>>,
    /// Last WebSocket activity timestamp (epoch seconds) - updated on ping/pong AND data messages
    last_ws_activity_epoch_secs: Arc<AtomicU64>,
}

impl KalshiConnector {
    /// Create a new Kalshi connector
    pub fn new(credentials: KalshiCredentials, use_demo: bool) -> Self {
        let (tx, rx) = mpsc::channel(1000);
        Self {
            credentials,
            use_demo,
            secmaster_config: None,
            subscription_config: SubscriptionConfig::default(),
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create connector with secmaster filtering enabled
    pub fn with_secmaster(
        credentials: KalshiCredentials,
        use_demo: bool,
        secmaster_config: SecmasterConfig,
        subscription_config: Option<SubscriptionConfig>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(1000);
        // Validate subscription config to clamp batch_size to valid range
        let (validated_config, was_clamped) = subscription_config.unwrap_or_default().validated();
        if was_clamped {
            tracing::warn!(
                batch_size = validated_config.batch_size,
                "batch_size was out of range and was clamped"
            );
        }
        Self {
            credentials,
            use_demo,
            secmaster_config: Some(secmaster_config),
            subscription_config: validated_config,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Subscribe globally to all markets (original behavior)
    async fn subscribe_global(&self, ws: &mut KalshiWebSocket) -> Result<(), ConnectorError> {
        info!("Using global subscription (all markets)");

        ws.subscribe_ticker()
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("ticker subscription: {}", e)))?;

        ws.subscribe_all_trades()
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("trade subscription: {}", e)))?;

        info!("Kalshi connector subscribed to all tickers and trades");
        Ok(())
    }

    /// Fetch filtered markets from secmaster
    async fn fetch_filtered_markets(
        &self,
        secmaster: &SecmasterConfig,
    ) -> Result<Vec<String>, ConnectorError> {
        info!(
            categories = ?secmaster.categories,
            close_within_hours = ?secmaster.close_within_hours,
            url = %secmaster.url,
            "Using filtered subscription mode"
        );

        // Fetch markets from secmaster with retry config and API key
        // Use env var SSMD_DATA_API_KEY as fallback if secmaster.api_key not set in config
        let api_key = secmaster
            .api_key
            .clone()
            .or_else(|| std::env::var("SSMD_DATA_API_KEY").ok());

        let client = SecmasterClient::with_config(
            &secmaster.url,
            api_key,
            self.subscription_config.retry_attempts,
            self.subscription_config.retry_delay_ms,
        );
        let tickers = client
            .get_markets_by_categories(&secmaster.categories, secmaster.close_within_hours)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("secmaster query: {}", e)))?;

        if tickers.is_empty() {
            return Err(ConnectorError::ConnectionFailed(format!(
                "No markets found for categories: {:?}",
                secmaster.categories
            )));
        }

        // Log market list at debug level
        debug!(markets = ?tickers, "Market ticker list");

        Ok(tickers)
    }

    /// Subscribe a single WebSocket to markets (up to MAX_MARKETS_PER_SUBSCRIPTION)
    async fn subscribe_shard(
        ws: &mut KalshiWebSocket,
        tickers: &[String],
    ) -> Result<(), ConnectorError> {
        // Subscribe to ticker channel for these markets
        ws.subscribe_markets("ticker", tickers)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("ticker subscription: {}", e)))?;

        // Subscribe to trade channel for these markets
        ws.subscribe_markets("trade", tickers)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("trade subscription: {}", e)))?;

        Ok(())
    }

    /// Spawn a WebSocket receiver task that forwards messages to the channel
    fn spawn_receiver_task(
        mut ws: KalshiWebSocket,
        tx: mpsc::Sender<Vec<u8>>,
        activity_tracker: Arc<AtomicU64>,
        shard_id: usize,
        shard_metrics: ShardMetrics,
    ) {
        // Helper to update activity timestamp and metrics
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

        tokio::spawn(async move {
            use std::time::Duration;
            use tokio::time::{interval, Instant};

            const PING_INTERVAL_SECS: u64 = 30;

            let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
            ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            // Track last activity for logging (local Instant for idle_secs calculation)
            let mut last_activity = Instant::now();

            // Initialize activity tracker with current time
            update_activity(&activity_tracker, &shard_metrics, 0.0);

            loop {
                tokio::select! {
                    // Ping timer fired - send keepalive
                    _ = ping_interval.tick() => {
                        let idle_secs = last_activity.elapsed().as_secs();
                        debug!(shard_id, idle_secs, "Sending WebSocket ping keepalive");
                        // Update idle seconds metric before ping
                        shard_metrics.set_idle_seconds(idle_secs as f64);
                        if let Err(e) = ws.ping().await {
                            error!(shard_id, error = %e, "Failed to send ping, connection may be dead");
                            shard_metrics.set_disconnected();
                            break;
                        }
                        // Ping succeeded - update activity tracker
                        update_activity(&activity_tracker, &shard_metrics, idle_secs as f64);
                    }

                    // Receive message from WebSocket
                    result = ws.recv_raw() => {
                        last_activity = Instant::now();
                        // Update activity tracker on any received message (including pongs)
                        update_activity(&activity_tracker, &shard_metrics, 0.0);

                        match result {
                            Ok((raw_json, msg)) => {
                                // Record metrics and determine if we should forward
                                let should_forward = match &msg {
                                    WsMessage::Ticker { .. } => {
                                        shard_metrics.inc_ticker();
                                        true
                                    }
                                    WsMessage::Trade { .. } => {
                                        shard_metrics.inc_trade();
                                        true
                                    }
                                    WsMessage::OrderbookSnapshot { .. } | WsMessage::OrderbookDelta { .. } => {
                                        shard_metrics.inc_orderbook();
                                        true
                                    }
                                    _ => false,
                                };

                                if !should_forward {
                                    continue;
                                }

                                // Pass through raw Kalshi JSON bytes - no re-serialization
                                if tx.send(raw_json.into_bytes()).await.is_err() {
                                    info!(shard_id, "Channel closed, stopping receiver");
                                    shard_metrics.set_disconnected();
                                    break;
                                }
                            }
                            Err(WebSocketError::ConnectionClosed) => {
                                info!(shard_id, "Kalshi WebSocket connection closed");
                                shard_metrics.set_disconnected();
                                break;
                            }
                            Err(e) => {
                                error!(shard_id, error = %e, "Kalshi WebSocket error");
                                shard_metrics.set_disconnected();
                                break;
                            }
                        }
                    }
                }
            }

            // Try to close gracefully
            if let Err(e) = ws.close().await {
                error!(shard_id, error = %e, "Error closing Kalshi WebSocket");
            }
        });
    }
}

#[async_trait]
impl Connector for KalshiConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        // Take the sender for the spawned tasks
        let tx = self.tx.take().ok_or_else(|| {
            ConnectorError::ConnectionFailed("connect() called twice".to_string())
        })?;

        // Clone activity tracker for the spawned tasks
        let activity_tracker = Arc::clone(&self.last_ws_activity_epoch_secs);

        // Determine subscription mode and get markets
        if let Some(ref secmaster) = self.secmaster_config {
            if !secmaster.categories.is_empty() {
                // Filtered mode: fetch markets and create sharded connections
                let tickers = self.fetch_filtered_markets(secmaster).await?;

                // Create metrics for this connector (use first category as label)
                let category_label = secmaster.categories.first()
                    .map(|s| s.to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                let connector_metrics = ConnectorMetrics::new("kalshi", &category_label);

                // Shard markets into groups of MAX_MARKETS_PER_SUBSCRIPTION
                let shards: Vec<Vec<String>> = tickers
                    .chunks(MAX_MARKETS_PER_SUBSCRIPTION)
                    .map(|chunk| chunk.to_vec())
                    .collect();

                let num_shards = shards.len();
                connector_metrics.set_shards_total(num_shards);

                info!(
                    total_markets = tickers.len(),
                    num_shards = num_shards,
                    max_per_shard = MAX_MARKETS_PER_SUBSCRIPTION,
                    "Creating sharded WebSocket connections"
                );

                // Create a WebSocket connection for each shard
                for (shard_id, shard_tickers) in shards.into_iter().enumerate() {
                    info!(
                        shard_id = shard_id,
                        markets = shard_tickers.len(),
                        "Connecting shard"
                    );

                    // Record markets per shard
                    connector_metrics.set_markets_subscribed(shard_id, shard_tickers.len());

                    let mut ws = KalshiWebSocket::connect(&self.credentials, self.use_demo)
                        .await
                        .map_err(|e| ConnectorError::ConnectionFailed(format!(
                            "shard {} connection: {}", shard_id, e
                        )))?;

                    Self::subscribe_shard(&mut ws, &shard_tickers).await.map_err(|e| {
                        ConnectorError::ConnectionFailed(format!(
                            "shard {} subscription: {}", shard_id, e
                        ))
                    })?;

                    info!(
                        shard_id = shard_id,
                        markets = shard_tickers.len(),
                        "Shard connected and subscribed"
                    );

                    // Create shard-specific metrics handle
                    let shard_metrics = connector_metrics.for_shard(shard_id);

                    // Spawn receiver task for this shard
                    Self::spawn_receiver_task(
                        ws,
                        tx.clone(),
                        Arc::clone(&activity_tracker),
                        shard_id,
                        shard_metrics,
                    );
                }

                info!(
                    total_markets = tickers.len(),
                    num_shards = num_shards,
                    "All shards connected"
                );
            } else {
                // Global mode: single connection
                let connector_metrics = ConnectorMetrics::new("kalshi", "global");
                connector_metrics.set_shards_total(1);
                connector_metrics.set_markets_subscribed(0, 0); // Unknown market count in global mode

                let mut ws = KalshiWebSocket::connect(&self.credentials, self.use_demo)
                    .await
                    .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

                self.subscribe_global(&mut ws).await?;
                let shard_metrics = connector_metrics.for_shard(0);
                Self::spawn_receiver_task(ws, tx, activity_tracker, 0, shard_metrics);
            }
        } else {
            // No secmaster config: global mode with single connection
            let connector_metrics = ConnectorMetrics::new("kalshi", "global");
            connector_metrics.set_shards_total(1);
            connector_metrics.set_markets_subscribed(0, 0);

            let mut ws = KalshiWebSocket::connect(&self.credentials, self.use_demo)
                .await
                .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

            self.subscribe_global(&mut ws).await?;
            let shard_metrics = connector_metrics.for_shard(0);
            Self::spawn_receiver_task(ws, tx, activity_tracker, 0, shard_metrics);
        }

        Ok(())
    }

    fn messages(&mut self) -> mpsc::Receiver<Vec<u8>> {
        self.rx.take().expect("messages() called before connect() or called twice")
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

    fn create_test_credentials() -> Result<KalshiCredentials, crate::kalshi::auth::AuthError> {
        use rsa::RsaPrivateKey;
        use rsa::pkcs8::EncodePrivateKey;

        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 2048).expect("failed to generate key");
        let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).expect("failed to encode");

        KalshiCredentials::new("test-api-key".to_string(), pem.as_str())
    }

    #[test]
    fn test_connector_creation() {
        let credentials = create_test_credentials().unwrap();
        let connector = KalshiConnector::new(credentials, true);

        assert!(connector.tx.is_some());
        assert!(connector.rx.is_some());
        assert!(connector.use_demo);
    }

    #[test]
    fn test_connector_messages_takes_receiver() {
        let credentials = create_test_credentials().unwrap();
        let mut connector = KalshiConnector::new(credentials, true);

        let _rx = connector.messages();
        assert!(connector.rx.is_none());
    }
}
