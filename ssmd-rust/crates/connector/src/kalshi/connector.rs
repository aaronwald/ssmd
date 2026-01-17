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
//! ## CDC Dynamic Updates
//!
//! When CDC is enabled, the connector subscribes to the SECMASTER_CDC stream and
//! dynamically adds new market subscriptions when markets are inserted. The shard
//! manager routes new subscriptions to shards with available capacity.

use crate::error::ConnectorError;
use crate::kalshi::auth::KalshiCredentials;
use crate::kalshi::cdc_consumer::{CdcConfig as CdcConsumerConfig, CdcSubscriptionConsumer};
use crate::kalshi::shard_manager::ShardManager;
use crate::kalshi::websocket::{KalshiWebSocket, WebSocketError, MAX_MARKETS_PER_SUBSCRIPTION};
use crate::kalshi::messages::WsMessage;
use crate::metrics::{ConnectorMetrics, ShardMetrics};
use crate::secmaster::SecmasterClient;
use crate::traits::Connector;
use async_trait::async_trait;
use ssmd_metadata::{CdcConfig, SecmasterConfig, SubscriptionConfig};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// Commands that can be sent to a shard's receiver task
#[derive(Debug)]
pub enum ShardCommand {
    /// Subscribe to additional markets on this shard
    Subscribe {
        /// Market tickers to subscribe to
        tickers: Vec<String>,
    },
}

/// Kalshi connector implementing the ssmd Connector trait
pub struct KalshiConnector {
    credentials: KalshiCredentials,
    use_demo: bool,
    secmaster_config: Option<SecmasterConfig>,
    subscription_config: SubscriptionConfig,
    /// CDC configuration for dynamic subscriptions
    cdc_config: Option<CdcConfig>,
    /// NATS URL for CDC (from transport config)
    nats_url: Option<String>,
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
            cdc_config: None,
            nats_url: None,
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
            cdc_config: None,
            nats_url: None,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create connector with secmaster filtering and CDC dynamic subscriptions
    pub fn with_cdc(
        credentials: KalshiCredentials,
        use_demo: bool,
        secmaster_config: SecmasterConfig,
        subscription_config: Option<SubscriptionConfig>,
        cdc_config: CdcConfig,
        nats_url: String,
    ) -> Self {
        let (tx, rx) = mpsc::channel(1000);
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
            cdc_config: Some(cdc_config),
            nats_url: Some(nats_url),
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
    ///
    /// Optionally accepts a command receiver for dynamic subscription updates (CDC).
    fn spawn_receiver_task(
        mut ws: KalshiWebSocket,
        tx: mpsc::Sender<Vec<u8>>,
        activity_tracker: Arc<AtomicU64>,
        shard_id: usize,
        shard_metrics: ShardMetrics,
        mut cmd_rx: Option<mpsc::Receiver<ShardCommand>>,
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
                        trace!(shard_id, idle_secs, "Sending WebSocket ping keepalive");
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

                    // Handle shard commands (dynamic subscriptions from CDC)
                    cmd = async {
                        match cmd_rx.as_mut() {
                            Some(rx) => rx.recv().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        match cmd {
                            Some(ShardCommand::Subscribe { tickers }) => {
                                info!(
                                    shard_id,
                                    market_count = tickers.len(),
                                    "CDC: Adding dynamic market subscriptions"
                                );

                                // Subscribe to ticker channel
                                if let Err(e) = ws.subscribe_markets("ticker", &tickers).await {
                                    warn!(shard_id, error = %e, "Failed to subscribe to ticker channel");
                                    // Continue - don't break the receiver loop
                                }

                                // Subscribe to trade channel
                                if let Err(e) = ws.subscribe_markets("trade", &tickers).await {
                                    warn!(shard_id, error = %e, "Failed to subscribe to trade channel");
                                }

                                // Update metrics
                                let current_count = shard_metrics.get_markets_subscribed();
                                shard_metrics.set_markets_subscribed(current_count + tickers.len());

                                info!(
                                    shard_id,
                                    added = tickers.len(),
                                    total = current_count + tickers.len(),
                                    "CDC: Successfully subscribed to new markets"
                                );
                            }
                            None => {
                                // Command channel closed - CDC disabled or shutting down
                                debug!(shard_id, "Command channel closed");
                                // Don't break - continue receiving WebSocket messages
                                cmd_rx = None;
                            }
                        }
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

                // Check if CDC is enabled
                let cdc_enabled = self.cdc_config.as_ref().map_or(false, |c| c.enabled);

                info!(
                    total_markets = tickers.len(),
                    num_shards = num_shards,
                    max_per_shard = MAX_MARKETS_PER_SUBSCRIPTION,
                    cdc_enabled = cdc_enabled,
                    "Creating sharded WebSocket connections"
                );

                // Create shard manager if CDC is enabled
                let mut shard_manager = if cdc_enabled {
                    Some(ShardManager::new(tickers.clone()))
                } else {
                    None
                };

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

                    // Create command channel for CDC if enabled
                    let cmd_rx = if let Some(ref mut manager) = shard_manager {
                        let (cmd_tx, cmd_rx) = mpsc::channel::<ShardCommand>(100);
                        manager.register_shard(shard_id, cmd_tx, shard_tickers.len());
                        Some(cmd_rx)
                    } else {
                        None
                    };

                    // Spawn receiver task for this shard
                    Self::spawn_receiver_task(
                        ws,
                        tx.clone(),
                        Arc::clone(&activity_tracker),
                        shard_id,
                        shard_metrics,
                        cmd_rx,
                    );
                }

                info!(
                    total_markets = tickers.len(),
                    num_shards = num_shards,
                    "All shards connected"
                );

                // Start CDC consumer if enabled
                if let (Some(cdc_config), Some(manager), Some(ref nats_url)) =
                    (&self.cdc_config, shard_manager, &self.nats_url)
                {
                    if cdc_config.enabled {
                        let consumer_name = cdc_config.consumer_name.clone()
                            .unwrap_or_else(|| format!("{}-cdc", category_label));

                        let cdc_nats_url = cdc_config.nats_url.clone()
                            .unwrap_or_else(|| nats_url.clone());

                        let cdc_consumer_config = CdcConsumerConfig {
                            nats_url: cdc_nats_url,
                            stream_name: cdc_config.stream_name.clone(),
                            consumer_name,
                            secmaster_url: secmaster.url.clone(),
                            secmaster_api_key: secmaster.api_key.clone(),
                        };

                        let categories = secmaster.categories.clone();

                        // Create channel for CDC → ShardManager communication
                        let (new_market_tx, new_market_rx) = mpsc::channel::<String>(1000);

                        // Spawn CDC consumer task
                        let snapshot_lsn = "0/0".to_string(); // TODO: Get actual LSN from initial fetch
                        let initial_markets = tickers.clone();
                        tokio::spawn(async move {
                            match CdcSubscriptionConsumer::new(
                                &cdc_consumer_config,
                                categories,
                                snapshot_lsn,
                                initial_markets,
                            ).await {
                                Ok(consumer) => {
                                    if let Err(e) = consumer.run(new_market_tx).await {
                                        error!(error = %e, "CDC consumer error");
                                    }
                                }
                                Err(e) => {
                                    error!(error = %e, "Failed to start CDC consumer");
                                }
                            }
                        });

                        // Spawn shard manager dispatcher task
                        tokio::spawn(async move {
                            manager.run(new_market_rx).await;
                        });

                        info!("CDC dynamic subscription enabled");
                    }
                }
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
                Self::spawn_receiver_task(ws, tx, activity_tracker, 0, shard_metrics, None);
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
            Self::spawn_receiver_task(ws, tx, activity_tracker, 0, shard_metrics, None);
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
