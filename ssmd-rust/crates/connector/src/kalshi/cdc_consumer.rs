//! CDC consumer for dynamic market subscriptions
//!
//! Subscribes to SECMASTER_CDC stream and sends new market tickers
//! for subscription when they match the configured categories.

use crate::secmaster::SecmasterClient;
use async_nats::jetstream::{self, consumer::pull::Stream, Context};
use futures_util::StreamExt;
use std::collections::HashSet;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Errors from CDC consumer operations
#[derive(Error, Debug)]
pub enum CdcError {
    #[error("NATS error: {0}")]
    Nats(String),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Secmaster error: {0}")]
    Secmaster(String),
}

/// CDC event from NATS (matches ssmd-cdc publisher format)
#[derive(Debug, serde::Deserialize)]
pub struct CdcEvent {
    pub lsn: String,
    pub table: String,
    pub op: String, // "insert", "update", "delete"
    pub key: serde_json::Value,
    pub data: Option<serde_json::Value>,
}

/// Market data from CDC event
#[derive(Debug, serde::Deserialize)]
struct MarketData {
    ticker: String,
    event_ticker: String,
}

/// CDC consumer configuration
#[derive(Debug, Clone)]
pub struct CdcConfig {
    /// NATS URL (e.g., "nats://nats.nats:4222")
    pub nats_url: String,
    /// JetStream stream name (default: "SECMASTER_CDC")
    pub stream_name: String,
    /// Durable consumer name (should be unique per connector instance)
    pub consumer_name: String,
    /// Secmaster API URL for category lookups
    pub secmaster_url: String,
    /// Secmaster API key (optional)
    pub secmaster_api_key: Option<String>,
}

/// CDC consumer for dynamic market subscriptions
pub struct CdcSubscriptionConsumer {
    stream: Stream,
    snapshot_lsn: String,
    secmaster_client: SecmasterClient,
    /// Categories to filter by (empty = all markets)
    categories: HashSet<String>,
    /// Already subscribed markets (to prevent duplicates)
    subscribed_markets: HashSet<String>,
}

impl CdcSubscriptionConsumer {
    /// Create a new CDC consumer
    ///
    /// # Arguments
    /// * `config` - CDC configuration
    /// * `categories` - Categories to filter by (empty = all markets)
    /// * `snapshot_lsn` - LSN from initial market fetch (skip events before this)
    /// * `initial_markets` - Markets already subscribed at startup
    pub async fn new(
        config: &CdcConfig,
        categories: Vec<String>,
        snapshot_lsn: String,
        initial_markets: Vec<String>,
    ) -> Result<Self, CdcError> {
        let client = async_nats::connect(&config.nats_url)
            .await
            .map_err(|e| CdcError::Nats(format!("Connection failed: {}", e)))?;
        let js: Context = jetstream::new(client);

        // Get stream
        let stream_obj = js
            .get_stream(&config.stream_name)
            .await
            .map_err(|e| CdcError::Nats(format!("Get stream '{}' failed: {}", config.stream_name, e)))?;

        // Create durable consumer for market inserts only
        let consumer = stream_obj
            .get_or_create_consumer(
                &config.consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(config.consumer_name.clone()),
                    // Only listen to market insert events
                    filter_subject: "cdc.markets.insert".to_string(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| CdcError::Nats(format!("Create consumer failed: {}", e)))?;

        let messages = consumer
            .messages()
            .await
            .map_err(|e| CdcError::Nats(format!("Get messages failed: {}", e)))?;

        let secmaster_client = SecmasterClient::with_config(
            &config.secmaster_url,
            config.secmaster_api_key.clone(),
            3,    // retry attempts
            1000, // retry delay ms
        );

        Ok(Self {
            stream: messages,
            snapshot_lsn,
            secmaster_client,
            categories: categories.into_iter().collect(),
            subscribed_markets: initial_markets.into_iter().collect(),
        })
    }

    /// Compare LSNs (format: "0/16B3748")
    fn lsn_gte(lsn: &str, threshold: &str) -> bool {
        lsn >= threshold
    }

    /// Check if a market's event category matches our filter
    async fn should_subscribe(&self, event_ticker: &str) -> bool {
        // If no categories configured, subscribe to all
        if self.categories.is_empty() {
            return true;
        }

        // Look up the event to get its category
        match self.secmaster_client.get_event(event_ticker).await {
            Ok(Some(event)) => {
                let matches = self.categories.contains(&event.category);
                debug!(
                    event_ticker = %event_ticker,
                    category = %event.category,
                    matches = matches,
                    "Category lookup"
                );
                matches
            }
            Ok(None) => {
                warn!(event_ticker = %event_ticker, "Event not found, skipping market");
                false
            }
            Err(e) => {
                warn!(
                    event_ticker = %event_ticker,
                    error = %e,
                    "Failed to lookup event, skipping market"
                );
                false
            }
        }
    }

    /// Run the CDC consumer, sending new market tickers to the channel
    ///
    /// This method runs indefinitely, processing CDC events and sending
    /// qualifying market tickers through the provided channel.
    pub async fn run(mut self, new_market_tx: mpsc::Sender<String>) -> Result<(), CdcError> {
        info!(
            snapshot_lsn = %self.snapshot_lsn,
            categories = ?self.categories,
            initial_markets = self.subscribed_markets.len(),
            "Starting CDC subscription consumer"
        );

        let mut processed: u64 = 0;
        let mut skipped_lsn: u64 = 0;
        let mut skipped_category: u64 = 0;
        let mut skipped_duplicate: u64 = 0;
        let mut subscribed: u64 = 0;

        while let Some(msg) = self.stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    error!(error = %e, "Error receiving message, will retry");
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            let event: CdcEvent = match serde_json::from_slice(&msg.payload) {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "Failed to parse CDC event");
                    if let Err(e) = msg.ack().await {
                        warn!(error = %e, "Failed to ack message");
                    }
                    continue;
                }
            };

            processed += 1;

            // Skip events before our snapshot LSN
            if !Self::lsn_gte(&event.lsn, &self.snapshot_lsn) {
                skipped_lsn += 1;
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "Failed to ack message");
                }
                continue;
            }

            // Extract market data
            let market_data: MarketData = match event.data {
                Some(data) => match serde_json::from_value(data) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(error = %e, "Failed to parse market data");
                        if let Err(e) = msg.ack().await {
                            warn!(error = %e, "Failed to ack message");
                        }
                        continue;
                    }
                },
                None => {
                    warn!("CDC event has no data");
                    if let Err(e) = msg.ack().await {
                        warn!(error = %e, "Failed to ack message");
                    }
                    continue;
                }
            };

            // Skip already subscribed markets
            if self.subscribed_markets.contains(&market_data.ticker) {
                skipped_duplicate += 1;
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "Failed to ack message");
                }
                continue;
            }

            // Check if market's event category matches our filter
            if !self.should_subscribe(&market_data.event_ticker).await {
                skipped_category += 1;
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "Failed to ack message");
                }
                continue;
            }

            // Send ticker for subscription
            info!(
                ticker = %market_data.ticker,
                event_ticker = %market_data.event_ticker,
                "CDC: New market for subscription"
            );

            if new_market_tx.send(market_data.ticker.clone()).await.is_err() {
                error!("Channel closed, stopping CDC consumer");
                break;
            }

            self.subscribed_markets.insert(market_data.ticker);
            subscribed += 1;

            // Ack the message
            if let Err(e) = msg.ack().await {
                warn!(error = %e, "Failed to ack message");
            }

            // Log progress periodically
            if processed.is_multiple_of(100) {
                info!(
                    processed,
                    skipped_lsn,
                    skipped_category,
                    skipped_duplicate,
                    subscribed,
                    "CDC consumer progress"
                );
            }
        }

        info!(
            processed,
            skipped_lsn,
            skipped_category,
            skipped_duplicate,
            subscribed,
            "CDC consumer stopped"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsn_comparison() {
        assert!(CdcSubscriptionConsumer::lsn_gte("0/16B3748", "0/16B3748"));
        assert!(CdcSubscriptionConsumer::lsn_gte("0/16B3749", "0/16B3748"));
        assert!(!CdcSubscriptionConsumer::lsn_gte("0/16B3747", "0/16B3748"));
        assert!(CdcSubscriptionConsumer::lsn_gte("1/0", "0/FFFFFF"));
    }

    #[test]
    fn test_cdc_event_parse() {
        let json = r#"{
            "lsn": "0/16B3748",
            "table": "markets",
            "op": "insert",
            "key": {"ticker": "KXTEST-123"},
            "data": {
                "ticker": "KXTEST-123",
                "event_ticker": "KXEVENT-1"
            }
        }"#;

        let event: CdcEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.table, "markets");
        assert_eq!(event.op, "insert");

        let market: MarketData = serde_json::from_value(event.data.unwrap()).unwrap();
        assert_eq!(market.ticker, "KXTEST-123");
        assert_eq!(market.event_ticker, "KXEVENT-1");
    }
}
