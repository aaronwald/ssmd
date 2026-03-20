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
use crate::kalshi::shard_manager::{ShardEvent, ShardManager};
use crate::kalshi::websocket::{KalshiWebSocket, WebSocketError, MAX_MARKETS_PER_SUBSCRIPTION};
use crate::kalshi::messages::WsMessage;
use crate::metrics::{ConnectorMetrics, ShardMetrics};
use crate::secmaster::SecmasterClient;
use crate::traits::{Connector, TimestampedMsg};
use async_trait::async_trait;
use ssmd_metadata::{CdcConfig, LifecycleConfig, SecmasterConfig, SubscriptionConfig};
use ssmd_middleware::now_tsc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::{debug, error, info, trace, warn};

/// Commands that can be sent to a shard's receiver task
#[derive(Debug)]
pub enum ShardCommand {
    /// Subscribe to additional markets on this shard
    Subscribe {
        /// Market tickers to subscribe to
        tickers: Vec<String>,
    },
    /// Unsubscribe from markets on this shard
    Unsubscribe {
        /// Market tickers to unsubscribe from
        tickers: Vec<String>,
    },
}

/// Kalshi connector implementing the ssmd Connector trait
pub struct KalshiConnector {
    credentials: KalshiCredentials,
    use_demo: bool,
    /// WebSocket URL override from feed config (None = use default constant)
    ws_url: Option<String>,
    secmaster_config: Option<SecmasterConfig>,
    subscription_config: SubscriptionConfig,
    /// CDC configuration for dynamic subscriptions
    cdc_config: Option<CdcConfig>,
    /// Lifecycle channel configuration
    lifecycle_config: Option<LifecycleConfig>,
    /// NATS URL for CDC (from transport config)
    nats_url: Option<String>,
    tx: Option<mpsc::Sender<TimestampedMsg>>,
    rx: Option<mpsc::Receiver<TimestampedMsg>>,
    /// Last WebSocket activity timestamp (epoch seconds) - updated ONLY on received data/pong, never on ping send
    last_ws_activity_epoch_secs: Arc<AtomicU64>,
    /// Background tasks (shard receivers, CDC consumer, shard manager).
    /// Monitored by the runner — any exit or panic triggers a crash instead of silent data loss.
    task_set: Option<JoinSet<()>>,
}

/// Handle a shard command (subscribe/unsubscribe). Used by both the blocking recv
/// arm and the periodic drain arm in the receiver task's select! loop.
macro_rules! handle_shard_cmd {
    ($ws:expr, $cmd:expr, $shard_id:expr, $metrics:expr) => {
        match $cmd {
            ShardCommand::Subscribe { tickers } => {
                info!(
                    shard_id = $shard_id,
                    market_count = tickers.len(),
                    "CDC: Adding dynamic market subscriptions"
                );
                if let Err(e) = $ws.subscribe_markets("ticker", &tickers).await {
                    warn!(shard_id = $shard_id, error = %e, "Failed to subscribe to ticker channel");
                }
                if let Err(e) = $ws.subscribe_markets("trade", &tickers).await {
                    warn!(shard_id = $shard_id, error = %e, "Failed to subscribe to trade channel");
                }
                let current_count = $metrics.get_markets_subscribed();
                $metrics.set_markets_subscribed(current_count + tickers.len());
                info!(
                    shard_id = $shard_id,
                    added = tickers.len(),
                    total = current_count + tickers.len(),
                    "CDC: Successfully subscribed to new markets"
                );
            }
            ShardCommand::Unsubscribe { tickers } => {
                info!(
                    shard_id = $shard_id,
                    market_count = tickers.len(),
                    "CDC: Removing market subscriptions"
                );
                let mut removed = 0usize;
                for ticker in &tickers {
                    match $ws.unsubscribe_market(ticker).await {
                        Ok(n) => {
                            removed += 1;
                            debug!(shard_id = $shard_id, ticker = %ticker, sids = n, "Unsubscribed market");
                            $metrics.inc_unsubscribed();
                        }
                        Err(e) => {
                            warn!(shard_id = $shard_id, ticker = %ticker, error = %e, "Failed to unsubscribe market");
                        }
                    }
                }
                let current_count = $metrics.get_markets_subscribed();
                $metrics.set_markets_subscribed(current_count.saturating_sub(removed));
                info!(
                    shard_id = $shard_id,
                    requested = tickers.len(),
                    removed,
                    total = current_count.saturating_sub(removed),
                    "CDC: Unsubscribe complete"
                );
            }
        }
    };
}

impl KalshiConnector {
    /// Create a new Kalshi connector
    pub fn new(credentials: KalshiCredentials, use_demo: bool, ws_url: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel(1000);
        Self {
            credentials,
            use_demo,
            ws_url,
            secmaster_config: None,
            subscription_config: SubscriptionConfig::default(),
            cdc_config: None,
            lifecycle_config: None,
            nats_url: None,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
            task_set: None,
        }
    }

    /// Create connector with secmaster filtering enabled
    pub fn with_secmaster(
        credentials: KalshiCredentials,
        use_demo: bool,
        secmaster_config: SecmasterConfig,
        subscription_config: Option<SubscriptionConfig>,
        ws_url: Option<String>,
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
            ws_url,
            secmaster_config: Some(secmaster_config),
            subscription_config: validated_config,
            cdc_config: None,
            lifecycle_config: None,
            nats_url: None,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
            task_set: None,
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
        ws_url: Option<String>,
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
            ws_url,
            secmaster_config: Some(secmaster_config),
            subscription_config: validated_config,
            cdc_config: Some(cdc_config),
            lifecycle_config: None,
            nats_url: Some(nats_url),
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
            task_set: None,
        }
    }

    /// Create connector for lifecycle events only (no market data)
    ///
    /// This is used for dedicated lifecycle collectors that subscribe
    /// only to the market_lifecycle_v2 channel.
    pub fn with_lifecycle(
        credentials: KalshiCredentials,
        use_demo: bool,
        lifecycle_config: LifecycleConfig,
        ws_url: Option<String>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(1000);
        Self {
            credentials,
            use_demo,
            ws_url,
            secmaster_config: None,
            subscription_config: SubscriptionConfig::default(),
            cdc_config: None,
            lifecycle_config: Some(lifecycle_config),
            nats_url: None,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
            task_set: None,
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

    /// Subscribe to lifecycle channel only (for dedicated lifecycle collector)
    async fn subscribe_lifecycle_only(&self, ws: &mut KalshiWebSocket) -> Result<(), ConnectorError> {
        info!("Using lifecycle-only subscription (market_lifecycle_v2)");

        ws.subscribe_lifecycle()
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("lifecycle subscription: {}", e)))?;

        info!("Kalshi connector subscribed to market lifecycle events");
        Ok(())
    }

    /// Fetch filtered markets from secmaster
    async fn fetch_filtered_markets(
        &self,
        secmaster: &SecmasterConfig,
    ) -> Result<Vec<String>, ConnectorError> {
        let result = self.fetch_filtered_markets_with_snapshot(secmaster).await?;
        Ok(result.tickers)
    }

    /// Fetch filtered markets from secmaster with CDC snapshot metadata
    /// Returns tickers plus snapshot_time and snapshot_lsn for CDC synchronization
    async fn fetch_filtered_markets_with_snapshot(
        &self,
        secmaster: &SecmasterConfig,
    ) -> Result<crate::secmaster::MarketsWithSnapshot, ConnectorError> {
        info!(
            categories = ?secmaster.categories,
            close_within_hours = ?secmaster.close_within_hours,
            url = %secmaster.url,
            "Using filtered subscription mode with CDC snapshot"
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
        let result = client
            .get_markets_by_categories_with_snapshot(&secmaster.categories, secmaster.close_within_hours, secmaster.games_only)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("secmaster query: {}", e)))?;

        if result.tickers.is_empty() {
            return Err(ConnectorError::ConnectionFailed(format!(
                "No markets found for categories: {:?}",
                secmaster.categories
            )));
        }

        // Log market list at debug level
        debug!(markets = ?result.tickers, "Market ticker list");

        Ok(result)
    }

    /// Subscribe a single WebSocket to markets (up to MAX_MARKETS_PER_SUBSCRIPTION)
    async fn subscribe_shard(
        ws: &mut KalshiWebSocket,
        tickers: &[String],
    ) -> Result<(), ConnectorError> {
        // Subscribe to ticker channel for these markets
        ws.subscribe_markets("ticker", tickers)
            .await
            .map(|_| ())
            .map_err(|e| ConnectorError::ConnectionFailed(format!("ticker subscription: {}", e)))?;

        // Subscribe to trade channel for these markets
        ws.subscribe_markets("trade", tickers)
            .await
            .map(|_| ())
            .map_err(|e| ConnectorError::ConnectionFailed(format!("trade subscription: {}", e)))?;

        Ok(())
    }

    /// Spawn a WebSocket receiver task that forwards messages to the channel.
    ///
    /// The task is added to the provided JoinSet so the runner can detect exits/panics
    /// instead of silently losing data from fire-and-forget spawns.
    /// Optionally accepts a command receiver for dynamic subscription updates (CDC).
    fn spawn_receiver_task(
        task_set: &mut JoinSet<()>,
        mut ws: KalshiWebSocket,
        tx: mpsc::Sender<TimestampedMsg>,
        activity_tracker: Arc<AtomicU64>,
        shard_id: usize,
        shard_metrics: ShardMetrics,
        mut cmd_rx: Option<mpsc::Receiver<ShardCommand>>,
    ) {
        // Helper to update activity timestamp and metrics.
        // Called ONLY when recv_raw returns data to the caller (not on ping send).
        fn update_activity(tracker: &AtomicU64, metrics: &ShardMetrics, idle_secs: f64) {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock before UNIX_EPOCH")
                .as_secs();
            tracker.store(now, Ordering::SeqCst);
            metrics.set_last_activity(now as f64);
            metrics.set_idle_seconds(idle_secs);
        }

        // Get pong tracker — updated inside recv_raw when Pong frames arrive.
        // This is the only way to detect liveness on idle connections where
        // recv_raw never returns data to the caller (all messages are pongs).
        let pong_tracker = ws.pong_tracker();

        // Mark shard as connected
        shard_metrics.set_connected();

        task_set.spawn(async move {
            use std::time::Duration;
            use tokio::time::{interval, Instant};

            const PING_INTERVAL_SECS: u64 = 30;
            const CMD_CHECK_INTERVAL_SECS: u64 = 5;
            /// If no data or pong received for this long, the connection is dead.
            /// Must be > PING_INTERVAL_SECS to allow at least one ping/pong round-trip.
            const DEFAULT_RECV_STALENESS_SECS: u64 = 90;
            let recv_staleness_secs: u64 = std::env::var("RECV_STALENESS_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_RECV_STALENESS_SECS);

            let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
            ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            let mut cmd_check_interval = interval(Duration::from_secs(CMD_CHECK_INTERVAL_SECS));
            cmd_check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            let connected_at = Instant::now();
            let mut message_count: u64 = 0;
            let shard_suffix = format!(",\"_shard_id\":{}}}", shard_id).into_bytes();

            // Track last activity for logging (local Instant for idle_secs calculation)
            let mut last_activity = Instant::now();

            // Initialize activity tracker with current time
            update_activity(&activity_tracker, &shard_metrics, 0.0);

            loop {
                tokio::select! {
                    // Ping timer fired - send keepalive and check for stale connection
                    _ = ping_interval.tick() => {
                        let idle_secs = last_activity.elapsed().as_secs();
                        debug!(shard_id, idle_secs, "Sending WebSocket ping keepalive");
                        shard_metrics.set_idle_seconds(idle_secs as f64);

                        // Check if we've received ANY data or pong recently.
                        // A successful ping send only means the OS TCP buffer accepted it —
                        // it does NOT prove the remote end is alive. Only received data or
                        // pong proves liveness. We check two sources:
                        //   - last_activity: updated when recv_raw returns data to this loop
                        //   - pong_tracker: updated inside recv_raw when Pong frames arrive
                        //     (pongs don't bubble up to the caller, they're handled internally)
                        // Connection is alive if EITHER source is recent.
                        let last_pong_epoch = pong_tracker.load(Ordering::SeqCst);
                        let now_epoch = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .expect("system clock before UNIX_EPOCH")
                            .as_secs();
                        let pong_age_secs = if last_pong_epoch > 0 {
                            now_epoch.saturating_sub(last_pong_epoch)
                        } else {
                            // No pong received yet — use idle_secs (time since connect/last data)
                            idle_secs
                        };
                        let stale_secs = idle_secs.min(pong_age_secs);

                        if stale_secs >= recv_staleness_secs {
                            let uptime_secs = connected_at.elapsed().as_secs();
                            error!(
                                shard_id,
                                idle_secs,
                                pong_age_secs,
                                uptime_secs,
                                message_count,
                                reason = "stale_connection",
                                "No data or pong received for {stale_secs}s (threshold {recv_staleness_secs}s), exiting for restart"
                            );
                            shard_metrics.set_disconnected();
                            std::process::exit(1);
                        }

                        // Wrap ping in a timeout — if the TCP send buffer is full
                        // (dead peer, kernel retransmitting), ping().await blocks
                        // indefinitely, which would prevent the staleness check
                        // above from ever running again.
                        match tokio::time::timeout(
                            Duration::from_secs(10),
                            ws.ping(),
                        ).await {
                            Ok(Ok(())) => {} // Ping sent (only means OS buffer accepted it)
                            Ok(Err(e)) => {
                                let uptime_secs = connected_at.elapsed().as_secs();
                                error!(
                                    shard_id,
                                    error = %e,
                                    uptime_secs,
                                    message_count,
                                    reason = "ping_failed",
                                    "Kalshi ping failed, exiting for restart"
                                );
                                shard_metrics.set_disconnected();
                                std::process::exit(1);
                            }
                            Err(_) => {
                                let uptime_secs = connected_at.elapsed().as_secs();
                                error!(
                                    shard_id,
                                    uptime_secs,
                                    message_count,
                                    reason = "ping_send_timeout",
                                    "Ping send timed out after 10s (TCP buffer full, peer likely dead), exiting for restart"
                                );
                                shard_metrics.set_disconnected();
                                std::process::exit(1);
                            }
                        }
                        // Do NOT update_activity here — ping send succeeding does not
                        // prove the connection is alive. Only received data/pong updates activity.
                    }

                    // Periodic drain of queued CDC commands during WS idle periods.
                    // Ensures subscriptions are processed even when no WS data arrives.
                    _ = cmd_check_interval.tick() => {
                        if let Some(rx) = cmd_rx.as_mut() {
                            while let Ok(cmd) = rx.try_recv() {
                                handle_shard_cmd!(&mut ws, cmd, shard_id, &shard_metrics);
                            }
                        }
                    }

                    // Handle shard commands (dynamic subscriptions from CDC)
                    cmd = async {
                        match cmd_rx.as_mut() {
                            Some(rx) => rx.recv().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        match cmd {
                            Some(cmd) => {
                                handle_shard_cmd!(&mut ws, cmd, shard_id, &shard_metrics);
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
                        match result {
                            Ok((raw_json, msg)) => {
                                // Only update activity on successful data receipt —
                                // NOT on errors. Updating on errors defeats the
                                // watchdog (resets staleness clock even when no data flows).
                                last_activity = Instant::now();
                                update_activity(&activity_tracker, &shard_metrics, 0.0);

                                message_count += 1;
                                trace!(shard_id, message_count, msg_type = %msg.type_str(), "WS recv");
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
                                    WsMessage::MarketLifecycleV2 { .. } => {
                                        shard_metrics.inc_lifecycle();
                                        true
                                    }
                                    WsMessage::EventLifecycle { .. } => {
                                        shard_metrics.inc_event_lifecycle();
                                        true
                                    }
                                    WsMessage::Subscribed { .. } => {
                                        shard_metrics.inc_message("subscribed");
                                        false
                                    }
                                    WsMessage::Ok { .. } => {
                                        shard_metrics.inc_message("ok");
                                        false
                                    }
                                    WsMessage::Unsubscribed { .. } => {
                                        shard_metrics.inc_message("unsubscribed");
                                        false
                                    }
                                    WsMessage::Error { .. } => {
                                        shard_metrics.inc_message("error");
                                        false
                                    }
                                    WsMessage::Unknown => {
                                        shard_metrics.inc_message("unknown");
                                        false
                                    }
                                };

                                if !should_forward {
                                    continue;
                                }

                                // Inject _shard_id into the JSON envelope before forwarding.
                                let mut forwarded = raw_json.into_bytes();
                                if forwarded.last() == Some(&b'}') {
                                    forwarded.pop();
                                    forwarded.extend_from_slice(&shard_suffix);
                                }
                                if tx.send((now_tsc(), forwarded)).await.is_err() {
                                    info!(shard_id, "Channel closed, stopping receiver");
                                    shard_metrics.set_disconnected();
                                    break;
                                }
                            }
                            Err(WebSocketError::ParseFailed { error, raw_json }) => {
                                shard_metrics.inc_parse_error();
                                warn!(shard_id, error = %error, "Parse failed, forwarding raw JSON to NATS");
                                // Still forward the raw JSON to NATS — don't lose data
                                let mut forwarded = raw_json.into_bytes();
                                if forwarded.last() == Some(&b'}') {
                                    forwarded.pop();
                                    forwarded.extend_from_slice(&shard_suffix);
                                }
                                if tx.send((now_tsc(), forwarded)).await.is_err() {
                                    info!(shard_id, "Channel closed, stopping receiver");
                                    shard_metrics.set_disconnected();
                                    break;
                                }
                                continue;
                            }
                            Err(e) => {
                                let uptime_secs = connected_at.elapsed().as_secs();
                                let subscribed_count = ws.subscribed_markets().len();
                                let last_pong = ws.pong_tracker().load(std::sync::atomic::Ordering::Relaxed);
                                let pong_age_secs = if last_pong > 0 {
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs()
                                        .saturating_sub(last_pong)
                                } else {
                                    0 // never received a pong
                                };
                                let reason = match &e {
                                    WebSocketError::ConnectionClosed => "connection_closed",
                                    WebSocketError::Connection(_) => "read_timeout",
                                    WebSocketError::SubscriptionFailed(_) => "subscription_failed",
                                    _ => "ws_error",
                                };
                                error!(
                                    shard_id,
                                    error = %e,
                                    uptime_secs,
                                    message_count,
                                    subscribed_count,
                                    pong_age_secs,
                                    reason,
                                    "Kalshi WebSocket disconnect, exiting for restart"
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
impl Connector for KalshiConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        // Take the sender for the spawned tasks
        let tx = self.tx.take().ok_or_else(|| {
            ConnectorError::ConnectionFailed("connect() called twice".to_string())
        })?;

        // Clone activity tracker for the spawned tasks
        let activity_tracker = Arc::clone(&self.last_ws_activity_epoch_secs);

        // JoinSet to track all background tasks — runner monitors for exits/panics
        let mut task_set = JoinSet::new();

        // Determine subscription mode and get markets
        if let Some(ref secmaster) = self.secmaster_config {
            if !secmaster.categories.is_empty() {
                // Check if CDC is enabled
                let cdc_enabled = self.cdc_config.as_ref().map_or(false, |c| c.enabled);

                // Filtered mode: fetch markets and create sharded connections
                // Use snapshot-aware fetch when CDC is enabled for proper synchronization
                let (tickers, snapshot_lsn, snapshot_time) = if cdc_enabled {
                    let result = self.fetch_filtered_markets_with_snapshot(secmaster).await?;
                    (result.tickers, result.snapshot_lsn, result.snapshot_time)
                } else {
                    let tickers = self.fetch_filtered_markets(secmaster).await?;
                    (tickers, "0/0".to_string(), String::new())
                };

                // Create metrics for this connector (use first category as label)
                let category_label = secmaster.categories.first()
                    .map(|s| s.to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                let connector_metrics = ConnectorMetrics::new("kalshi", &category_label);

                // Shard size for startup distribution. Defaults to 200 (vs 256 WS hard limit)
                // to leave headroom per shard for CDC market additions during contract rollovers.
                let shard_size = std::env::var("SHARD_SIZE")
                    .ok()
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(200)
                    .min(MAX_MARKETS_PER_SUBSCRIPTION); // never exceed WS limit

                let mut shards: Vec<Vec<String>> = tickers
                    .chunks(shard_size)
                    .map(|chunk| chunk.to_vec())
                    .collect();

                // When CDC is enabled, ensure at least one shard exists for new markets
                if cdc_enabled && shards.is_empty() {
                    shards.push(Vec::new());
                }

                let num_shards = shards.len();
                let total_capacity = num_shards * shard_size;
                let overflow = tickers.len().saturating_sub(total_capacity);
                connector_metrics.set_shards_total(num_shards);
                connector_metrics.set_shard_capacity(shard_size);
                connector_metrics.set_markets_requested(tickers.len());
                connector_metrics.set_markets_overflow(overflow);

                if overflow > 0 {
                    warn!(
                        total_markets = tickers.len(),
                        total_capacity = total_capacity,
                        overflow = overflow,
                        num_shards = num_shards,
                        shard_size = shard_size,
                        "SUBSCRIPTION OVERFLOW: {} markets cannot fit in {} shards — these markets will have no WS data",
                        overflow, num_shards,
                    );
                }

                info!(
                    total_markets = tickers.len(),
                    num_shards = num_shards,
                    shard_size = shard_size,
                    max_per_shard = MAX_MARKETS_PER_SUBSCRIPTION,
                    cdc_enabled = cdc_enabled,
                    snapshot_lsn = %snapshot_lsn,
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

                    let mut ws = KalshiWebSocket::connect(&self.credentials, self.use_demo, self.ws_url.as_deref())
                        .await
                        .map_err(|e| ConnectorError::ConnectionFailed(format!(
                            "shard {} connection: {}", shard_id, e
                        )))?;

                    // Only subscribe if shard has initial markets (headroom shards start empty)
                    if !shard_tickers.is_empty() {
                        Self::subscribe_shard(&mut ws, &shard_tickers).await.map_err(|e| {
                            ConnectorError::ConnectionFailed(format!(
                                "shard {} subscription: {}", shard_id, e
                            ))
                        })?;
                    }

                    info!(
                        shard_id = shard_id,
                        markets = shard_tickers.len(),
                        "Shard connected and subscribed"
                    );

                    // Create shard-specific metrics handle
                    let shard_metrics = connector_metrics.for_shard(shard_id);
                    shard_metrics.init(&["ticker", "trade", "orderbook", "lifecycle", "event_lifecycle"]);

                    // Create command channel for CDC if enabled
                    let cmd_rx = if let Some(ref mut manager) = shard_manager {
                        let (cmd_tx, cmd_rx) = mpsc::channel::<ShardCommand>(100);
                        manager.register_shard(shard_id, cmd_tx, shard_tickers.len());
                        // Record initial ticker-to-shard mappings
                        for ticker in &shard_tickers {
                            manager.record_ticker_shard(ticker, shard_id);
                        }
                        Some(cmd_rx)
                    } else {
                        None
                    };

                    // Spawn receiver task for this shard
                    Self::spawn_receiver_task(
                        &mut task_set,
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
                            .unwrap_or_else(|| format!("{}-cdc-v2", category_label));

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
                        let (new_market_tx, new_market_rx) = mpsc::channel::<ShardEvent>(1000);

                        // Spawn CDC consumer task with snapshot data from initial fetch
                        let cdc_snapshot_lsn = snapshot_lsn.clone();
                        let cdc_snapshot_time = snapshot_time.clone();
                        let initial_markets = tickers.clone();
                        task_set.spawn(async move {
                            match CdcSubscriptionConsumer::new(
                                &cdc_consumer_config,
                                categories,
                                cdc_snapshot_lsn,
                                cdc_snapshot_time,
                                initial_markets,
                            ).await {
                                Ok(consumer) => {
                                    if let Err(e) = consumer.run(new_market_tx).await {
                                        error!(error = %e, "CDC consumer error — exiting for restart");
                                        std::process::exit(1);
                                    }
                                }
                                Err(e) => {
                                    error!(error = %e, "Failed to start CDC consumer — exiting for restart");
                                    std::process::exit(1);
                                }
                            }
                        });

                        // Spawn shard manager dispatcher task
                        task_set.spawn(async move {
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

                let mut ws = KalshiWebSocket::connect(&self.credentials, self.use_demo, self.ws_url.as_deref())
                    .await
                    .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

                self.subscribe_global(&mut ws).await?;
                let shard_metrics = connector_metrics.for_shard(0);
                shard_metrics.init(&["ticker", "trade", "orderbook", "lifecycle", "event_lifecycle"]);
                Self::spawn_receiver_task(&mut task_set, ws, tx, activity_tracker, 0, shard_metrics, None);
            }
        } else if let Some(ref lifecycle) = self.lifecycle_config {
            if lifecycle.enabled {
                // Lifecycle-only mode: single connection for market lifecycle events
                let connector_metrics = ConnectorMetrics::new("kalshi", "lifecycle");
                connector_metrics.set_shards_total(1);
                connector_metrics.set_markets_subscribed(0, 0);

                let mut ws = KalshiWebSocket::connect(&self.credentials, self.use_demo, self.ws_url.as_deref())
                    .await
                    .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

                self.subscribe_lifecycle_only(&mut ws).await?;
                let shard_metrics = connector_metrics.for_shard(0);
                shard_metrics.init(&["lifecycle", "event_lifecycle"]);
                Self::spawn_receiver_task(&mut task_set, ws, tx, activity_tracker, 0, shard_metrics, None);
            } else {
                return Err(ConnectorError::ConnectionFailed(
                    "Lifecycle config present but disabled".to_string()
                ));
            }
        } else {
            // No secmaster config: global mode with single connection
            let connector_metrics = ConnectorMetrics::new("kalshi", "global");
            connector_metrics.set_shards_total(1);
            connector_metrics.set_markets_subscribed(0, 0);

            let mut ws = KalshiWebSocket::connect(&self.credentials, self.use_demo, self.ws_url.as_deref())
                .await
                .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

            self.subscribe_global(&mut ws).await?;
            let shard_metrics = connector_metrics.for_shard(0);
            shard_metrics.init(&["ticker", "trade", "orderbook", "lifecycle", "event_lifecycle"]);
            Self::spawn_receiver_task(&mut task_set, ws, tx, activity_tracker, 0, shard_metrics, None);
        }

        self.task_set = Some(task_set);
        Ok(())
    }

    fn messages(&mut self) -> mpsc::Receiver<TimestampedMsg> {
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

    fn tasks(&mut self) -> Option<JoinSet<()>> {
        self.task_set.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_credentials() -> Result<KalshiCredentials, crate::kalshi::auth::AuthError> {
        use rsa::RsaPrivateKey;
        use rsa::pkcs8::EncodePrivateKey;

        let mut rng = rand_core::OsRng;
        let key = RsaPrivateKey::new(&mut rng, 2048).expect("failed to generate key");
        let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).expect("failed to encode");

        KalshiCredentials::new("test-api-key".to_string(), pem.as_str())
    }

    #[test]
    fn test_connector_creation() {
        let credentials = create_test_credentials().unwrap();
        let connector = KalshiConnector::new(credentials, true, None);

        assert!(connector.tx.is_some());
        assert!(connector.rx.is_some());
        assert!(connector.use_demo);
    }

    #[test]
    fn test_connector_messages_takes_receiver() {
        let credentials = create_test_credentials().unwrap();
        let mut connector = KalshiConnector::new(credentials, true, None);

        let _rx = connector.messages();
        assert!(connector.rx.is_none());
    }
}
