//! Shard manager for routing dynamic subscriptions
//!
//! Manages shard command channels and routes new market subscriptions
//! to shards with available capacity.

use crate::kalshi::connector::ShardCommand;
use crate::kalshi::websocket::MAX_MARKETS_PER_SUBSCRIPTION;
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Manages shards and routes dynamic subscriptions from CDC
pub struct ShardManager {
    /// Command senders for each shard, keyed by shard_id
    shard_commands: HashMap<usize, mpsc::Sender<ShardCommand>>,
    /// Market count per shard
    shard_market_counts: HashMap<usize, usize>,
    /// All subscribed market tickers (prevents duplicates)
    subscribed_markets: HashSet<String>,
    /// Batch size for subscription commands (accumulate before sending)
    batch_size: usize,
    /// Pending tickers waiting to be batched
    pending_tickers: Vec<String>,
}

impl ShardManager {
    /// Create a new shard manager
    ///
    /// # Arguments
    /// * `initial_markets` - Markets already subscribed at startup
    pub fn new(initial_markets: Vec<String>) -> Self {
        Self {
            shard_commands: HashMap::new(),
            shard_market_counts: HashMap::new(),
            subscribed_markets: initial_markets.into_iter().collect(),
            batch_size: 10, // Send subscriptions in batches of 10
            pending_tickers: Vec::new(),
        }
    }

    /// Register a shard's command channel
    ///
    /// Call this for each shard created during connector startup.
    pub fn register_shard(
        &mut self,
        shard_id: usize,
        cmd_tx: mpsc::Sender<ShardCommand>,
        initial_market_count: usize,
    ) {
        self.shard_commands.insert(shard_id, cmd_tx);
        self.shard_market_counts.insert(shard_id, initial_market_count);
        debug!(
            shard_id,
            market_count = initial_market_count,
            "Registered shard with manager"
        );
    }

    /// Get total number of registered shards
    pub fn shard_count(&self) -> usize {
        self.shard_commands.len()
    }

    /// Get total number of subscribed markets across all shards
    pub fn total_markets(&self) -> usize {
        self.subscribed_markets.len()
    }

    /// Check if a market is already subscribed
    pub fn is_subscribed(&self, ticker: &str) -> bool {
        self.subscribed_markets.contains(ticker)
    }

    /// Find a shard with available capacity
    fn find_shard_with_capacity(&self) -> Option<usize> {
        self.shard_market_counts
            .iter()
            .filter(|(shard_id, _)| self.shard_commands.contains_key(shard_id))
            .filter(|(_, &count)| count < MAX_MARKETS_PER_SUBSCRIPTION)
            .min_by_key(|(_, &count)| count)
            .map(|(&shard_id, _)| shard_id)
    }

    /// Get available capacity in a shard
    fn shard_capacity(&self, shard_id: usize) -> usize {
        self.shard_market_counts
            .get(&shard_id)
            .map(|&count| MAX_MARKETS_PER_SUBSCRIPTION.saturating_sub(count))
            .unwrap_or(0)
    }

    /// Add a market ticker for subscription
    ///
    /// Returns true if the ticker was added for subscription,
    /// false if already subscribed or no capacity.
    pub async fn add_subscription(&mut self, ticker: String) -> bool {
        // Skip if already subscribed
        if self.subscribed_markets.contains(&ticker) {
            debug!(ticker = %ticker, "Already subscribed, skipping");
            return false;
        }

        // Mark as subscribed (optimistic - prevents duplicate attempts)
        self.subscribed_markets.insert(ticker.clone());
        self.pending_tickers.push(ticker);

        // Flush if we've reached batch size
        if self.pending_tickers.len() >= self.batch_size {
            self.flush().await;
        }

        true
    }

    /// Flush pending subscriptions to shards
    pub async fn flush(&mut self) {
        if self.pending_tickers.is_empty() {
            return;
        }

        let tickers = std::mem::take(&mut self.pending_tickers);
        let mut remaining = tickers;

        while !remaining.is_empty() {
            // Find a shard with capacity
            let shard_id = match self.find_shard_with_capacity() {
                Some(id) => id,
                None => {
                    warn!(
                        pending = remaining.len(),
                        "All shards at capacity, cannot subscribe to new markets"
                    );
                    // Put remaining tickers back for later
                    self.pending_tickers.extend(remaining);
                    return;
                }
            };

            // Calculate how many we can send to this shard
            let capacity = self.shard_capacity(shard_id);
            let batch_count = remaining.len().min(capacity);
            let batch: Vec<String> = remaining.drain(..batch_count).collect();

            // Send subscribe command
            if let Some(cmd_tx) = self.shard_commands.get(&shard_id) {
                let cmd = ShardCommand::Subscribe {
                    tickers: batch.clone(),
                };

                match cmd_tx.send(cmd).await {
                    Ok(()) => {
                        info!(
                            shard_id,
                            count = batch.len(),
                            "Sent subscription batch to shard"
                        );
                        // Update market count
                        if let Some(count) = self.shard_market_counts.get_mut(&shard_id) {
                            *count += batch.len();
                        }
                    }
                    Err(e) => {
                        warn!(
                            shard_id,
                            error = %e,
                            "Failed to send subscription command, shard may be disconnected"
                        );
                        // Remove this shard from active shards
                        self.shard_commands.remove(&shard_id);
                        // Put tickers back for retry
                        self.pending_tickers.extend(batch);
                    }
                }
            }
        }
    }

    /// Run the shard manager dispatcher loop
    ///
    /// Receives market tickers from CDC and routes them to shards.
    pub async fn run(mut self, mut new_market_rx: mpsc::Receiver<String>) {
        info!(
            shard_count = self.shard_count(),
            initial_markets = self.total_markets(),
            "Starting shard manager dispatcher"
        );

        let mut ticker_count: u64 = 0;
        let mut flush_interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        flush_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // Receive new market ticker from CDC
                ticker = new_market_rx.recv() => {
                    match ticker {
                        Some(ticker) => {
                            ticker_count += 1;
                            self.add_subscription(ticker).await;
                        }
                        None => {
                            info!("CDC channel closed, flushing remaining subscriptions");
                            self.flush().await;
                            break;
                        }
                    }
                }

                // Periodic flush
                _ = flush_interval.tick() => {
                    self.flush().await;
                }
            }
        }

        info!(
            total_processed = ticker_count,
            total_subscribed = self.total_markets(),
            "Shard manager stopped"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_manager_creation() {
        let initial = vec!["MARKET-1".to_string(), "MARKET-2".to_string()];
        let manager = ShardManager::new(initial);

        assert_eq!(manager.total_markets(), 2);
        assert!(manager.is_subscribed("MARKET-1"));
        assert!(manager.is_subscribed("MARKET-2"));
        assert!(!manager.is_subscribed("MARKET-3"));
    }

    #[tokio::test]
    async fn test_shard_registration() {
        let mut manager = ShardManager::new(vec![]);
        let (tx, _rx) = mpsc::channel(10);

        manager.register_shard(0, tx.clone(), 100);
        manager.register_shard(1, tx, 200);

        assert_eq!(manager.shard_count(), 2);
    }

    #[test]
    fn test_find_shard_with_capacity() {
        let mut manager = ShardManager::new(vec![]);
        let (tx, _rx) = mpsc::channel::<ShardCommand>(10);

        // Shard 0 is full
        manager.register_shard(0, tx.clone(), MAX_MARKETS_PER_SUBSCRIPTION);
        // Shard 1 has capacity
        manager.register_shard(1, tx, 100);

        let shard = manager.find_shard_with_capacity();
        assert_eq!(shard, Some(1));
    }

    #[test]
    fn test_no_capacity_available() {
        let mut manager = ShardManager::new(vec![]);
        let (tx, _rx) = mpsc::channel::<ShardCommand>(10);

        // Both shards full
        manager.register_shard(0, tx.clone(), MAX_MARKETS_PER_SUBSCRIPTION);
        manager.register_shard(1, tx, MAX_MARKETS_PER_SUBSCRIPTION);

        let shard = manager.find_shard_with_capacity();
        assert_eq!(shard, None);
    }
}
