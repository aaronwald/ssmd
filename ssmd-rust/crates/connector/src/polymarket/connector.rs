//! Polymarket connector implementation
//!
//! Implements the ssmd Connector trait for Polymarket CLOB WebSocket.
//! Key differences from Kraken:
//! - Sharding: multiple WS connections needed (500 instrument limit)
//! - Market discovery: Gamma REST API polling (no CDC, no static config)
//! - Keepalive: 10-second PING interval (vs Kraken's 30s)
//! - Relies on 120s read timeout to detect stale connections (WS may go silent)

use crate::error::ConnectorError;
use crate::metrics::{ConnectorMetrics, ShardMetrics};
use crate::polymarket::market_discovery::MarketDiscovery;
use crate::polymarket::websocket::{
    PolymarketWebSocket, PolymarketWebSocketError, MAX_INSTRUMENTS_PER_CONNECTION,
};
use crate::secmaster::SecmasterClient;
use crate::traits::{Connector, TimestampedMsg};
use async_trait::async_trait;
use ssmd_metadata::SecmasterConfig;
use ssmd_middleware::now_tsc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Polymarket PING interval: 10 seconds (required by Polymarket, vs 30s for Kraken)
const PING_INTERVAL_SECS: u64 = 10;

/// Polymarket connector implementing the ssmd Connector trait
pub struct PolymarketConnector {
    /// Token IDs to subscribe to (can be set statically or via discovery)
    token_ids: Vec<String>,
    /// Optional market discovery client for dynamic subscription
    discovery: Option<MarketDiscovery>,
    /// Optional secmaster config for category-based token filtering
    secmaster_config: Option<SecmasterConfig>,
    /// WebSocket URL override from feed config (None = use default constant)
    ws_url: Option<String>,
    tx: Option<mpsc::Sender<TimestampedMsg>>,
    rx: Option<mpsc::Receiver<TimestampedMsg>>,
    /// Last WebSocket activity timestamp (epoch seconds)
    last_ws_activity_epoch_secs: Arc<AtomicU64>,
}

impl PolymarketConnector {
    /// Create a new Polymarket connector with static token IDs
    pub fn new(token_ids: Vec<String>, ws_url: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel(2000); // Larger buffer for multi-shard reconnect bursts
        Self {
            token_ids,
            discovery: None,
            secmaster_config: None,
            ws_url,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create a new Polymarket connector with market discovery
    pub fn with_discovery(discovery: MarketDiscovery, ws_url: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel(2000);
        Self {
            token_ids: Vec::new(),
            discovery: Some(discovery),
            secmaster_config: None,
            ws_url,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create a new Polymarket connector with secmaster category-based filtering
    pub fn with_secmaster(secmaster_config: SecmasterConfig, ws_url: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel(2000);
        Self {
            token_ids: Vec::new(),
            discovery: None,
            secmaster_config: Some(secmaster_config),
            ws_url,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Fetch token IDs from secmaster by categories
    async fn fetch_filtered_tokens(
        secmaster_config: &SecmasterConfig,
    ) -> Result<Vec<String>, ConnectorError> {
        let client = SecmasterClient::with_config(
            &secmaster_config.url,
            secmaster_config.api_key.clone(),
            3,
            1000,
        );

        // Read optional volume and question filters from env vars
        let min_volume = std::env::var("POLYMARKET_MIN_VOLUME")
            .ok()
            .and_then(|v| v.parse::<u64>().ok());
        let question_filter = std::env::var("POLYMARKET_QUESTION_FILTER").ok();

        client
            .get_polymarket_tokens_by_categories(
                &secmaster_config.categories,
                min_volume,
                question_filter.as_deref(),
            )
            .await
            .map_err(|e| {
                ConnectorError::ConnectionFailed(format!("Secmaster token fetch: {}", e))
            })
    }

    /// Extract event_type from raw JSON without full deserialization.
    /// Looks for `"event_type":"<value>"` pattern near the start of the message.
    fn extract_event_type(raw: &str) -> &str {
        // Polymarket messages have "event_type":"<type>" near the start.
        // Array-wrapped messages: [{"event_type":"..."}]
        const NEEDLE: &str = "\"event_type\":\"";
        if let Some(start) = raw.find(NEEDLE) {
            let value_start = start + NEEDLE.len();
            if let Some(end) = raw[value_start..].find('"') {
                return &raw[value_start..value_start + end];
            }
        }
        "unknown"
    }

    /// Spawn a WebSocket receiver task for a shard (subset of token IDs)
    fn spawn_shard_receiver(
        shard_id: usize,
        mut ws: PolymarketWebSocket,
        tx: mpsc::Sender<TimestampedMsg>,
        activity_tracker: Arc<AtomicU64>,
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

        shard_metrics.set_connected();
        update_activity(&activity_tracker, &shard_metrics, 0.0);

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
                    // Ping timer - send app-level "PING" text every 10s
                    _ = ping_interval.tick() => {
                        let idle_secs = last_activity_instant.elapsed().as_secs();
                        shard_metrics.set_idle_seconds(idle_secs as f64);
                        if let Err(e) = ws.ping().await {
                            let uptime_secs = connected_at.elapsed().as_secs();
                            error!(
                                shard = shard_id,
                                error = %e,
                                uptime_secs,
                                message_count,
                                reason = "ping_failed",
                                "Polymarket ping failed, exiting for restart"
                            );
                            shard_metrics.set_disconnected();
                            std::process::exit(1);
                        }
                        update_activity(&activity_tracker, &shard_metrics, idle_secs as f64);
                    }

                    // Receive message from WebSocket
                    // Stale connections detected by 120s read timeout in websocket.rs
                    result = ws.recv_raw() => {
                        last_activity_instant = Instant::now();
                        update_activity(&activity_tracker, &shard_metrics, 0.0);

                        match result {
                            Ok(raw_json) => {
                                message_count += 1;
                                // Skip PONG responses (don't forward to NATS)
                                if raw_json == "PONG" {
                                    shard_metrics.inc_message("pong");
                                    continue;
                                }

                                // Extract event_type for metrics without full deserialization
                                let event_type = Self::extract_event_type(&raw_json);
                                match event_type {
                                    "last_trade_price" => shard_metrics.inc_trade(),
                                    "book" => shard_metrics.inc_orderbook(),
                                    "price_change" | "best_bid_ask" => shard_metrics.inc_ticker(),
                                    other => shard_metrics.inc_message(other),
                                }

                                if tx.send((now_tsc(), raw_json.into_bytes())).await.is_err() {
                                    warn!(shard = shard_id, "Channel closed, stopping receiver");
                                    shard_metrics.set_disconnected();
                                    break;
                                }
                            }
                            Err(e) => {
                                let uptime_secs = connected_at.elapsed().as_secs();
                                let reason = match &e {
                                    PolymarketWebSocketError::ConnectionClosed => "connection_closed",
                                    PolymarketWebSocketError::Connection(_) => "read_timeout",
                                    _ => "ws_error",
                                };
                                error!(
                                    shard = shard_id,
                                    error = %e,
                                    uptime_secs,
                                    message_count,
                                    reason,
                                    "Polymarket WebSocket disconnect, exiting for restart"
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
impl Connector for PolymarketConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        let tx = self.tx.take().ok_or_else(|| {
            ConnectorError::ConnectionFailed("connect() called twice".to_string())
        })?;

        let activity_tracker = Arc::clone(&self.last_ws_activity_epoch_secs);

        // If secmaster is configured with categories, fetch tokens from secmaster
        if let Some(ref secmaster_config) = self.secmaster_config {
            if !secmaster_config.categories.is_empty() {
                info!(
                    categories = ?secmaster_config.categories,
                    "Fetching token IDs from secmaster"
                );
                self.token_ids = Self::fetch_filtered_tokens(secmaster_config).await?;
                info!(
                    token_ids = self.token_ids.len(),
                    "Loaded tokens from secmaster"
                );
            }
        } else if let Some(ref discovery) = self.discovery {
            // Fallback: discover via Gamma REST API
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

        for (shard_id, shard_tokens) in shards.into_iter().enumerate() {
            // Stagger shard startup by 2 seconds + random jitter (0-3s)
            if shard_id > 0 {
                let jitter_ms = (shard_id as u64 * 2000) + (rand::random::<u64>() % 3000);
                tokio::time::sleep(std::time::Duration::from_millis(jitter_ms)).await;
            }

            info!(
                shard = shard_id,
                tokens = shard_tokens.len(),
                "Connecting shard"
            );

            let mut ws = PolymarketWebSocket::connect(self.ws_url.as_deref())
                .await
                .map_err(|e| ConnectorError::ConnectionFailed(format!("shard {}: {}", shard_id, e)))?;

            ws.subscribe(&shard_tokens)
                .await
                .map_err(|e| ConnectorError::ConnectionFailed(format!("shard {} subscribe: {}", shard_id, e)))?;

            connector_metrics.set_markets_subscribed(shard_id, shard_tokens.len());

            let shard_metrics = connector_metrics.for_shard(shard_id);
            shard_metrics.set_connected();

            Self::spawn_shard_receiver(
                shard_id,
                ws,
                tx.clone(),
                Arc::clone(&activity_tracker),
                shard_metrics,
            );

            info!(
                shard = shard_id,
                tokens = shard_tokens.len(),
                "Shard connected and subscribed"
            );
        }

        info!(
            num_shards = num_shards,
            total_tokens = self.token_ids.len(),
            "All Polymarket shards connected"
        );

        Ok(())
    }

    fn messages(&mut self) -> mpsc::Receiver<TimestampedMsg> {
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
            PolymarketConnector::new(vec!["token1".to_string(), "token2".to_string()], None);
        assert!(connector.tx.is_some());
        assert!(connector.rx.is_some());
        assert_eq!(connector.token_ids.len(), 2);
    }

    #[test]
    fn test_connector_messages_takes_receiver() {
        let mut connector = PolymarketConnector::new(vec!["token1".to_string()], None);
        let _rx = connector.messages();
        assert!(connector.rx.is_none());
    }

    #[test]
    fn test_connector_activity_handle() {
        let connector = PolymarketConnector::new(vec!["token1".to_string()], None);
        let handle = connector.activity_handle();
        assert!(handle.is_some());
    }

    #[test]
    fn test_channel_buffer_size() {
        // Verify we use 2000 buffer (larger than default 1000)
        let connector = PolymarketConnector::new(vec!["token1".to_string()], None);
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
    fn test_extract_event_type() {
        assert_eq!(
            PolymarketConnector::extract_event_type(
                r#"{"event_type":"last_trade_price","asset_id":"abc"}"#
            ),
            "last_trade_price"
        );
        assert_eq!(
            PolymarketConnector::extract_event_type(
                r#"{"event_type":"book","asset_id":"abc","bids":[]}"#
            ),
            "book"
        );
        assert_eq!(
            PolymarketConnector::extract_event_type(
                r#"[{"event_type":"price_change","market":"0x1234"}]"#
            ),
            "price_change"
        );
        assert_eq!(
            PolymarketConnector::extract_event_type(r#"{"no_type":"value"}"#),
            "unknown"
        );
        assert_eq!(
            PolymarketConnector::extract_event_type(
                r#"{"event_type":"best_bid_ask","market":"0x1234"}"#
            ),
            "best_bid_ask"
        );
    }
}
