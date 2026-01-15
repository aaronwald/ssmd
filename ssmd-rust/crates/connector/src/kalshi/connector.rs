//! Kalshi connector implementation
//!
//! Implements the ssmd Connector trait for Kalshi WebSocket.
//!
//! ## Subscription Modes
//!
//! - **Global mode**: Subscribes to all markets (original behavior)
//! - **Filtered mode**: Subscribes only to markets in configured categories
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
use crate::kalshi::websocket::{KalshiWebSocket, WebSocketError};
use crate::kalshi::messages::WsMessage;
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

    /// Subscribe to filtered markets from secmaster
    async fn subscribe_filtered(
        &self,
        ws: &mut KalshiWebSocket,
        secmaster: &SecmasterConfig,
    ) -> Result<(), ConnectorError> {
        info!(
            categories = ?secmaster.categories,
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
            .get_markets_by_categories(&secmaster.categories)
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

        // Subscribe to ticker channel for these markets
        ws.subscribe_markets("ticker", &tickers)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("ticker subscription: {}", e)))?;

        // Subscribe to trade channel for these markets
        ws.subscribe_markets("trade", &tickers)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("trade subscription: {}", e)))?;

        info!(
            total_markets = tickers.len(),
            "Subscription complete"
        );

        Ok(())
    }
}

#[async_trait]
impl Connector for KalshiConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        let mut ws = KalshiWebSocket::connect(&self.credentials, self.use_demo)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        // Determine subscription mode
        if let Some(ref secmaster) = self.secmaster_config {
            if !secmaster.categories.is_empty() {
                self.subscribe_filtered(&mut ws, secmaster).await?;
            } else {
                self.subscribe_global(&mut ws).await?;
            }
        } else {
            self.subscribe_global(&mut ws).await?;
        }

        // Take the sender for the spawned task
        let tx = self.tx.take().ok_or_else(|| {
            ConnectorError::ConnectionFailed("connect() called twice".to_string())
        })?;

        // Clone activity tracker for the spawned task
        let activity_tracker = Arc::clone(&self.last_ws_activity_epoch_secs);

        // Helper to update activity timestamp
        fn update_activity(tracker: &AtomicU64) {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            tracker.store(now, Ordering::SeqCst);
        }

        // Spawn task to receive messages and forward to channel
        // Pass through raw Kalshi JSON bytes for data messages
        // Also sends periodic pings to detect dead connections
        tokio::spawn(async move {
            use std::time::Duration;
            use tokio::time::{interval, Instant};

            const PING_INTERVAL_SECS: u64 = 30;

            let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
            ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            // Track last activity for logging (local Instant for idle_secs calculation)
            let mut last_activity = Instant::now();

            // Initialize activity tracker with current time
            update_activity(&activity_tracker);

            loop {
                tokio::select! {
                    // Ping timer fired - send keepalive
                    _ = ping_interval.tick() => {
                        let idle_secs = last_activity.elapsed().as_secs();
                        debug!(idle_secs, "Sending WebSocket ping keepalive");
                        if let Err(e) = ws.ping().await {
                            error!(error = %e, "Failed to send ping, connection may be dead");
                            break;
                        }
                        // Ping succeeded - update activity tracker
                        update_activity(&activity_tracker);
                    }

                    // Receive message from WebSocket
                    result = ws.recv_raw() => {
                        last_activity = Instant::now();
                        // Update activity tracker on any received message (including pongs)
                        update_activity(&activity_tracker);

                        match result {
                            Ok((raw_json, msg)) => {
                                // Skip control messages, pass through data messages as raw bytes
                                let should_forward = matches!(
                                    msg,
                                    WsMessage::Ticker { .. }
                                        | WsMessage::Trade { .. }
                                        | WsMessage::OrderbookSnapshot { .. }
                                        | WsMessage::OrderbookDelta { .. }
                                );

                                if !should_forward {
                                    continue;
                                }

                                // Pass through raw Kalshi JSON bytes - no re-serialization
                                if tx.send(raw_json.into_bytes()).await.is_err() {
                                    info!("Channel closed, stopping receiver");
                                    break;
                                }
                            }
                            Err(WebSocketError::ConnectionClosed) => {
                                info!("Kalshi WebSocket connection closed");
                                break;
                            }
                            Err(e) => {
                                error!(error = %e, "Kalshi WebSocket error");
                                break;
                            }
                        }
                    }
                }
            }

            // Try to close gracefully
            if let Err(e) = ws.close().await {
                error!(error = %e, "Error closing Kalshi WebSocket");
            }
        });

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
