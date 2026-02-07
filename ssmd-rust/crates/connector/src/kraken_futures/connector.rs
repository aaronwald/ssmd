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
use tracing::{debug, error, info, trace, warn};

use super::messages::KrakenFuturesWsMessage;
use super::websocket::{KrakenFuturesWebSocket, KrakenFuturesWsError, PING_INTERVAL_SECS};
use crate::error::ConnectorError;
use crate::traits::Connector;

pub struct KrakenFuturesConnector {
    product_ids: Vec<String>,
    tx: Option<mpsc::Sender<Vec<u8>>>,
    rx: Option<mpsc::Receiver<Vec<u8>>>,
    last_ws_activity_epoch_secs: Arc<AtomicU64>,
}

impl KrakenFuturesConnector {
    pub fn new(product_ids: Vec<String>) -> Self {
        let (tx, rx) = mpsc::channel(4096);
        Self {
            product_ids,
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
        update_activity(&last_activity);

        tokio::spawn(async move {
            use std::time::Duration;
            use tokio::time::interval;

            let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
            ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    // Ping timer
                    _ = ping_interval.tick() => {
                        trace!("Sending Kraken Futures ping");
                        if let Err(e) = ws.ping().await {
                            error!(error = %e, "Failed to send Kraken Futures ping");
                            break;
                        }
                        update_activity(&last_activity);
                    }

                    // Receive message from WebSocket
                    result = ws.recv_raw() => {
                        update_activity(&last_activity);

                        match result {
                            Ok((raw, msg)) => {
                                match &msg {
                                    KrakenFuturesWsMessage::DataMessage { feed, product_id, .. } => {
                                        debug!(feed = %feed, product_id = %product_id, "Kraken Futures data message");
                                        if tx.send(raw.into_bytes()).await.is_err() {
                                            warn!("Channel closed, exiting receiver");
                                            return;
                                        }
                                    }
                                    KrakenFuturesWsMessage::Heartbeat { .. } => {
                                        trace!("Kraken Futures heartbeat received");
                                    }
                                    KrakenFuturesWsMessage::Error { message, .. } => {
                                        warn!(message = ?message, "Kraken Futures WS error");
                                    }
                                    _ => {
                                        // Info, subscribed — skip
                                    }
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "Kraken Futures WS error, exiting");
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }

            // Try to close gracefully
            if let Err(e) = ws.close().await {
                error!(error = %e, "Error closing Kraken Futures WebSocket");
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

        // Connect to Kraken Futures WS
        let mut ws = KrakenFuturesWebSocket::connect()
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
            KrakenFuturesConnector::new(vec!["PF_XBTUSD".to_string(), "PF_ETHUSD".to_string()]);
        assert!(connector.tx.is_some());
        assert!(connector.rx.is_some());
        assert_eq!(connector.product_ids.len(), 2);
    }

    #[test]
    fn test_connector_messages_takes_receiver() {
        let mut connector = KrakenFuturesConnector::new(vec!["PF_XBTUSD".to_string()]);
        let _rx = connector.messages();
        assert!(connector.rx.is_none());
    }

    #[test]
    fn test_connector_activity_handle() {
        let connector = KrakenFuturesConnector::new(vec!["PF_XBTUSD".to_string()]);
        let handle = connector.activity_handle();
        assert!(handle.is_some());
    }
}
