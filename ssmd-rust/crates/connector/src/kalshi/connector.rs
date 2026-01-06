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

        // Fetch markets from secmaster with retry config
        let client = SecmasterClient::with_retry(
            &secmaster.url,
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

        let batch_size = self.subscription_config.batch_size;

        // Subscribe to ticker channel for these markets
        let ticker_result = ws
            .subscribe_markets("ticker", &tickers, batch_size)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("ticker subscription: {}", e)))?;

        // Subscribe to trade channel for these markets
        let trade_result = ws
            .subscribe_markets("trade", &tickers, batch_size)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("trade subscription: {}", e)))?;

        info!(
            ticker_subs = ticker_result.successful,
            trade_subs = trade_result.successful,
            total_markets = tickers.len(),
            ticker_failed = ticker_result.failed,
            trade_failed = trade_result.failed,
            "Subscription complete"
        );

        // Fail if no subscriptions succeeded
        if ticker_result.successful == 0 && trade_result.successful == 0 {
            return Err(ConnectorError::ConnectionFailed(
                "All subscriptions failed".to_string(),
            ));
        }

        // Warn about partial failures (some markets subscribed, some failed)
        let total_failed = ticker_result.failed + trade_result.failed;
        if total_failed > 0 {
            tracing::warn!(
                ticker_failed = ticker_result.failed,
                trade_failed = trade_result.failed,
                failed_tickers = ?ticker_result.failed_tickers,
                "Some subscriptions failed - continuing with partial coverage"
            );
        }

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

        // Spawn task to receive messages and forward to channel
        // Pass through raw Kalshi JSON bytes for data messages
        tokio::spawn(async move {
            loop {
                match ws.recv_raw().await {
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
