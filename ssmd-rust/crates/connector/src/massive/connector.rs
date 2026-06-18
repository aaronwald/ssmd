//! Polygon.io ("massive") connector implementation
//!
//! Implements the ssmd Connector trait for the Polygon.io delayed equities
//! WebSocket cluster (`wss://delayed.polygon.io/stocks`).
//!
//! Authentication is via an auth frame (not HTTP headers). On any WebSocket
//! error the spawned receiver task logs and exits (`std::process::exit(1)`)
//! so K8s restarts the pod — no reconnect-and-hope.

use crate::error::ConnectorError;
use crate::massive::websocket::MassiveWebSocket;
use crate::traits::{Connector, TimestampedMsg};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

/// Polygon.io delayed-cluster connector implementing the ssmd `Connector` trait.
pub struct MassiveConnector {
    api_key: String,
    symbols: Vec<String>,
    /// Optional WebSocket URL override (None = use `MASSIVE_WS_DELAYED_URL`)
    url: Option<String>,
    tx: Option<mpsc::Sender<TimestampedMsg>>,
    rx: Option<mpsc::Receiver<TimestampedMsg>>,
    /// Last WebSocket activity timestamp (epoch seconds).
    last_ws_activity_epoch_secs: Arc<AtomicU64>,
}

impl MassiveConnector {
    /// Create a new connector.
    ///
    /// * `api_key`  — Polygon.io API key (from `MASSIVE_API_KEY` env var)
    /// * `symbols`  — list of equity tickers to subscribe (T.* and Q.* channels)
    /// * `url`      — optional WebSocket URL override (for tests / staging)
    pub fn new(api_key: String, symbols: Vec<String>, url: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel(10_000);
        Self {
            api_key,
            symbols,
            url,
            tx: Some(tx),
            rx: Some(rx),
            last_ws_activity_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Number of symbols this connector is configured to track.
    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    /// Update the epoch-seconds activity tracker.
    fn update_activity(tracker: &AtomicU64) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        tracker.store(now, Ordering::SeqCst);
    }
}

#[async_trait]
impl Connector for MassiveConnector {
    /// Open the WebSocket, authenticate, subscribe, then spawn the receiver task.
    ///
    /// The spawned task forwards every `(tsc, bytes)` from `ws.recv()` to the
    /// mpsc channel and bumps `last_ws_activity_epoch_secs` on each message.
    ///
    /// On `Err` from `recv()` the task logs the error and calls
    /// `std::process::exit(1)` — K8s restarts the pod. Do NOT reconnect.
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        let tx = self.tx.take().ok_or_else(|| {
            ConnectorError::ConnectionFailed("connect() called twice".to_string())
        })?;

        let mut ws = MassiveWebSocket::connect(self.url.as_deref())
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        ws.authenticate(&self.api_key)
            .await
            .map_err(|e| ConnectorError::AuthFailed(e.to_string()))?;

        info!(
            symbols = ?self.symbols,
            count = self.symbols.len(),
            "Subscribing to Massive (Polygon.io) T.* and Q.* channels"
        );

        ws.subscribe(&self.symbols)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        info!(
            subscribed = self.symbols.len(),
            "Massive connector subscribed — spawning receiver task"
        );

        let activity_tracker = Arc::clone(&self.last_ws_activity_epoch_secs);
        // Initialise activity so the pod doesn't look stale immediately.
        Self::update_activity(&activity_tracker);

        tokio::spawn(async move {
            let mut ws = ws;
            loop {
                match ws.recv().await {
                    Ok(Some((tsc, bytes))) => {
                        Self::update_activity(&activity_tracker);
                        if tx.send((tsc, bytes)).await.is_err() {
                            // Receiver dropped — runner is shutting down.
                            info!("Massive receiver channel closed, stopping task");
                            break;
                        }
                    }
                    Ok(None) => {
                        // Server sent a Close frame — unexpected during operation, fail loud.
                        error!("Massive WebSocket closed by server — crashing for restart");
                        std::process::exit(1);
                    }
                    Err(e) => {
                        // Protocol error — fail loud so K8s restarts the pod.
                        error!(error = %e, "Massive WebSocket error, exiting for pod restart");
                        std::process::exit(1);
                    }
                }
            }
        });

        Ok(())
    }

    fn messages(&mut self) -> mpsc::Receiver<TimestampedMsg> {
        self.rx
            .take()
            .expect("messages() called before connect() or called twice")
    }

    async fn close(&mut self) -> Result<(), ConnectorError> {
        // Drop the sender to signal the spawned receiver task to stop.
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
    fn new_stores_symbols_and_key() {
        let c = MassiveConnector::new("KEY".into(), vec!["AAPL".into()], None);
        assert_eq!(c.symbol_count(), 1);
    }

    #[test]
    fn new_creates_channel() {
        let c = MassiveConnector::new("K".into(), vec!["SPY".into()], None);
        assert!(c.tx.is_some());
        assert!(c.rx.is_some());
    }

    #[test]
    fn messages_takes_receiver() {
        let mut c = MassiveConnector::new("K".into(), vec!["SPY".into()], None);
        let _rx = c.messages();
        assert!(c.rx.is_none());
    }

    #[test]
    fn activity_handle_returns_some() {
        let c = MassiveConnector::new("K".into(), vec!["SPY".into()], None);
        assert!(c.activity_handle().is_some());
    }

    #[test]
    fn symbol_count_reflects_input() {
        let syms: Vec<String> = vec!["A", "B", "C"].iter().map(|s| s.to_string()).collect();
        let c = MassiveConnector::new("K".into(), syms, None);
        assert_eq!(c.symbol_count(), 3);
    }

    #[test]
    fn url_override_stored() {
        let c = MassiveConnector::new(
            "K".into(),
            vec!["SPY".into()],
            Some("ws://localhost:9999".into()),
        );
        assert_eq!(c.url.as_deref(), Some("ws://localhost:9999"));
    }
}
