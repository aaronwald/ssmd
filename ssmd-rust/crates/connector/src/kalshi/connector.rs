//! Kalshi connector implementation
//!
//! Implements the ssmd Connector trait for Kalshi WebSocket.

use crate::error::ConnectorError;
use crate::kalshi::auth::KalshiCredentials;
use crate::kalshi::websocket::{KalshiWebSocket, WebSocketError};
use crate::kalshi::messages::WsMessage;
use crate::traits::Connector;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::{error, info};

/// Kalshi connector implementing the ssmd Connector trait
pub struct KalshiConnector {
    credentials: KalshiCredentials,
    use_demo: bool,
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
            tx: Some(tx),
            rx: Some(rx),
        }
    }
}

#[async_trait]
impl Connector for KalshiConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        let mut ws = KalshiWebSocket::connect(&self.credentials, self.use_demo)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        // Subscribe to ticker and all trades
        ws.subscribe_ticker()
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("ticker subscription: {}", e)))?;

        ws.subscribe_all_trades()
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(format!("trade subscription: {}", e)))?;

        info!("Kalshi connector subscribed to ticker and trades");

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
