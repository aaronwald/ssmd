//! CDC consumer for dynamic market subscriptions
//!
//! Subscribes to SECMASTER_CDC stream and sends new market tickers
//! for subscription when they match the configured categories.

use crate::kalshi::shard_manager::ShardEvent;
use crate::secmaster::SecmasterClient;
use async_nats::jetstream::{self, consumer::pull::Stream, consumer::DeliverPolicy, AckKind, Context};
use chrono::Duration as ChronoDuration;
use futures_util::StreamExt;
use ssmd_middleware::lsn_gte;
use std::collections::{HashMap, HashSet};
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
    #[serde(default)]
    status: Option<String>,
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
    /// Feed label for metrics (e.g. "kalshi")
    pub feed: String,
    /// Category label for metrics (e.g. "crypto")
    pub category: String,
    /// Optional series-suffix filter (e.g. "15M"). When set, markets are selected by
    /// matching the series part of the ticker against this suffix instead of by an HTTP
    /// category lookup — eliminating the per-market secmaster call (and its 429 risk).
    pub series_suffix: Option<String>,
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
    /// Cache of event_ticker -> category to avoid one HTTP lookup per market.
    /// A single event has hundreds of strike markets that all resolve to the
    /// same category; caching collapses that to one lookup per event and
    /// prevents the secmaster API from being flooded (429) during bursts of
    /// new markets (e.g. 15-minute crypto series rolling every 15 minutes).
    event_category_cache: HashMap<String, String>,
    /// Feed label for metrics (e.g. "kalshi")
    feed: String,
    /// Category label for metrics (e.g. "crypto")
    category: String,
    /// Optional series-suffix filter (e.g. "15M") — see [`CdcConfig::series_suffix`].
    series_suffix: Option<String>,
}

/// Match a market's event ticker against a series suffix.
///
/// The series ticker is the segment before the first `-` (e.g. `KXBTC15M` in
/// `KXBTC15M-26JUN031400`). Returns true if that segment ends with `suffix`.
fn series_matches_suffix(event_ticker: &str, suffix: &str) -> bool {
    event_ticker
        .split('-')
        .next()
        .map_or(false, |series| series.ends_with(suffix))
}

/// Decision for whether to subscribe to a market based on its event category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscribeDecision {
    /// Event category matches the filter — subscribe.
    Subscribe,
    /// Event category is known and does not match — skip permanently (ack).
    SkipCategory,
    /// Category is unknown because the lookup failed (429/5xx/network) or the
    /// event is not yet synced (404). Must retry — NEVER drop the market on a
    /// transient failure.
    Retry,
}

/// Decide subscription from a (possibly unknown) category and the filter set.
///
/// Pure function so the branching logic is unit-testable without HTTP/NATS.
/// `category == None` means the category could not be resolved → retry.
fn decide_from_category(category: Option<&str>, categories: &HashSet<String>) -> SubscribeDecision {
    // No filter configured → subscribe to everything.
    if categories.is_empty() {
        return SubscribeDecision::Subscribe;
    }
    match category {
        Some(cat) if categories.contains(cat) => SubscribeDecision::Subscribe,
        Some(_) => SubscribeDecision::SkipCategory,
        None => SubscribeDecision::Retry,
    }
}

/// Max entries in the event->category cache before it is cleared (defensive
/// bound on memory; categories never change so a full clear is harmless).
const MAX_CATEGORY_CACHE: usize = 50_000;

/// Backoff applied when NAKing a market whose category lookup failed.
const LOOKUP_RETRY_BACKOFF: Duration = Duration::from_secs(5);

/// Number of failed deliveries after which we start emitting ERROR logs (the
/// signal a CDC-failure alert keys on).
const ALERT_DELIVERY_THRESHOLD: i64 = 3;

/// Hard cap on redeliveries for a single market before we give up *loudly*
/// (ERROR log + ack). At LOOKUP_RETRY_BACKOFF this is ~100s of retries, which
/// is far longer than any transient secmaster outage should last.
const MAX_LOOKUP_RETRIES: i64 = 20;

/// Check if a market status is terminal (market will no longer produce data).
/// These statuses should trigger unsubscribe from the WebSocket.
fn is_terminal_status(status: &str) -> bool {
    matches!(status, "determined" | "settled" | "closed" | "finalized" | "deactivated")
}

/// Buffer time (seconds) to subtract from snapshot_time for CDC start position
/// This ensures we don't miss events due to clock skew between DB and NATS
const CDC_START_TIME_BUFFER_SECS: i64 = 120;

impl CdcSubscriptionConsumer {
    /// Create a new CDC consumer
    ///
    /// # Arguments
    /// * `config` - CDC configuration
    /// * `categories` - Categories to filter by (empty = all markets)
    /// * `snapshot_lsn` - LSN from initial market fetch (skip events before this)
    /// * `snapshot_time` - ISO timestamp from initial market fetch (for ByStartTime)
    /// * `initial_markets` - Markets already subscribed at startup
    pub async fn new(
        config: &CdcConfig,
        categories: Vec<String>,
        snapshot_lsn: String,
        snapshot_time: String,
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

        // Calculate start time for CDC consumer: snapshot_time minus buffer
        // This ensures we catch any events that might have occurred during the market fetch
        // LSN filtering will dedupe any events we've already processed
        let deliver_policy = if !snapshot_time.is_empty() {
            match chrono::DateTime::parse_from_rfc3339(&snapshot_time) {
                Ok(parsed) => {
                    let start_time = parsed - ChronoDuration::seconds(CDC_START_TIME_BUFFER_SECS);
                    info!(
                        snapshot_time = %snapshot_time,
                        start_time = %start_time.to_rfc3339(),
                        buffer_secs = CDC_START_TIME_BUFFER_SECS,
                        "Using ByStartTime for CDC consumer"
                    );
                    // Convert chrono DateTime to time::OffsetDateTime via SystemTime
                    let system_time = std::time::UNIX_EPOCH +
                        Duration::from_secs(start_time.timestamp() as u64);
                    DeliverPolicy::ByStartTime {
                        start_time: system_time.into(),
                    }
                }
                Err(e) => {
                    warn!(
                        snapshot_time = %snapshot_time,
                        error = %e,
                        "Failed to parse snapshot_time, falling back to DeliverPolicy::New"
                    );
                    DeliverPolicy::New
                }
            }
        } else {
            info!("No snapshot_time provided, using DeliverPolicy::New");
            DeliverPolicy::New
        };

        // Create durable consumer for market inserts only
        // Uses ByStartTime when snapshot available, falls back to New otherwise
        // LSN filtering provides precise deduplication
        let mut consumer = stream_obj
            .get_or_create_consumer(
                &config.consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(config.consumer_name.clone()),
                    // Listen to all market CDC events (insert, update, delete)
                    filter_subject: "cdc.markets.>".to_string(),
                    deliver_policy,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| CdcError::Nats(format!("Create consumer failed: {}", e)))?;

        // Log consumer info to show starting position
        let consumer_info = consumer
            .info()
            .await
            .map_err(|e| CdcError::Nats(format!("Get consumer info failed: {}", e)))?;
        info!(
            consumer_name = %config.consumer_name,
            stream = %config.stream_name,
            snapshot_lsn = %snapshot_lsn,
            delivered_stream_seq = consumer_info.delivered.stream_sequence,
            delivered_consumer_seq = consumer_info.delivered.consumer_sequence,
            ack_floor_stream_seq = consumer_info.ack_floor.stream_sequence,
            num_pending = consumer_info.num_pending,
            "CDC consumer starting position"
        );

        // Set heartbeat to 5s to detect stale connections
        let messages = consumer
            .stream()
            .heartbeat(Duration::from_secs(5))
            .messages()
            .await
            .map_err(|e| CdcError::Nats(format!("Get messages failed: {}", e)))?;

        let secmaster_client = SecmasterClient::with_config(
            &config.secmaster_url,
            config.secmaster_api_key.clone(),
            3,    // retry attempts
            1000, // retry delay ms
        );

        // Pre-initialize CDC metrics so GMP discovers them during healthy periods.
        crate::metrics::init_cdc_metrics(&config.feed, &config.category);

        Ok(Self {
            stream: messages,
            snapshot_lsn,
            secmaster_client,
            categories: categories.into_iter().collect(),
            subscribed_markets: initial_markets.into_iter().collect(),
            event_category_cache: HashMap::new(),
            feed: config.feed.clone(),
            category: config.category.clone(),
            series_suffix: config.series_suffix.clone(),
        })
    }

    /// Resolve whether to subscribe to a market, using a cached event->category
    /// map and falling back to a secmaster lookup on cache miss.
    ///
    /// Unlike the previous implementation, a failed lookup (429/5xx/network) or
    /// a not-yet-synced event (404) returns [`SubscribeDecision::Retry`] instead
    /// of silently dropping the market. The caller NAKs so JetStream redelivers.
    async fn resolve_decision(&mut self, event_ticker: &str) -> SubscribeDecision {
        // Series-suffix mode: deterministic ticker-pattern match, NO HTTP lookup (and thus no
        // 429/retry path). Subscribes any market whose series ends with the suffix.
        if let Some(suffix) = &self.series_suffix {
            return if series_matches_suffix(event_ticker, suffix) {
                SubscribeDecision::Subscribe
            } else {
                SubscribeDecision::SkipCategory
            };
        }

        // No filter configured → subscribe to all, no lookup needed.
        if self.categories.is_empty() {
            return SubscribeDecision::Subscribe;
        }

        // Cache hit — one HTTP lookup per event instead of per market.
        if let Some(category) = self.event_category_cache.get(event_ticker) {
            return decide_from_category(Some(category), &self.categories);
        }

        // Cache miss — look up the event's category.
        match self.secmaster_client.get_event(event_ticker).await {
            Ok(Some(event)) => {
                // Defensive: bound cache growth. Categories never change, so a
                // full clear only costs re-lookups, never correctness.
                if self.event_category_cache.len() >= MAX_CATEGORY_CACHE {
                    warn!(
                        size = self.event_category_cache.len(),
                        "Event category cache full, clearing"
                    );
                    self.event_category_cache.clear();
                }
                let decision = decide_from_category(Some(&event.category), &self.categories);
                debug!(
                    event_ticker = %event_ticker,
                    category = %event.category,
                    ?decision,
                    "Category lookup (cached)"
                );
                self.event_category_cache
                    .insert(event_ticker.to_string(), event.category);
                decision
            }
            Ok(None) => {
                // 404: market exists in CDC but its event is not in secmaster
                // yet (sync race). Retry rather than drop.
                crate::metrics::inc_cdc_lookup_failure(&self.feed, &self.category);
                debug!(
                    event_ticker = %event_ticker,
                    "Event not yet in secmaster, will retry (not dropping market)"
                );
                SubscribeDecision::Retry
            }
            Err(e) => {
                // Transient (429/5xx/network). Retry rather than drop.
                crate::metrics::inc_cdc_lookup_failure(&self.feed, &self.category);
                warn!(
                    event_ticker = %event_ticker,
                    error = %e,
                    "Event lookup failed, will retry (not dropping market)"
                );
                SubscribeDecision::Retry
            }
        }
    }

    /// NAK a message so JetStream redelivers it after a backoff, bounding total
    /// retries via the message's own delivery count. On the final attempt the
    /// market is given up *loudly* (ERROR log, then ack) — never silently.
    ///
    /// Returns `true` if the message was NAKed (caller should treat as retried),
    /// `false` if we gave up and acked.
    async fn nak_or_give_up(
        msg: &jetstream::Message,
        ticker: &str,
        event_ticker: &str,
        feed: &str,
        category: &str,
    ) -> bool {
        let delivered = msg.info().map(|i| i.delivered).unwrap_or(1);

        if delivered >= MAX_LOOKUP_RETRIES {
            crate::metrics::inc_cdc_market_dropped(feed, category);
            error!(
                ticker = %ticker,
                event_ticker = %event_ticker,
                delivered,
                "CDC: giving up subscribing market after repeated failed category lookups — \
                 market NOT subscribed (secmaster unavailable?)"
            );
            if let Err(e) = msg.ack().await {
                warn!(error = %e, "Failed to ack message after give-up");
            }
            return false;
        }

        if delivered >= ALERT_DELIVERY_THRESHOLD {
            error!(
                ticker = %ticker,
                event_ticker = %event_ticker,
                delivered,
                "CDC: market category lookup repeatedly failing, will retry"
            );
        }

        if let Err(e) = msg.ack_with(AckKind::Nak(Some(LOOKUP_RETRY_BACKOFF))).await {
            warn!(error = %e, "Failed to NAK message");
        }
        true
    }

    /// Run the CDC consumer, sending new market tickers to the channel
    ///
    /// This method runs indefinitely, processing CDC events and sending
    /// qualifying market tickers through the provided channel.
    pub async fn run(mut self, event_tx: mpsc::Sender<ShardEvent>) -> Result<(), CdcError> {
        warn!(
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
        let mut subscribed_update: u64 = 0;
        let mut skipped_delete: u64 = 0;
        let mut skipped_update_inactive: u64 = 0;
        let mut unsubscribed: u64 = 0;
        let mut retried: u64 = 0;
        let mut dropped_after_retries: u64 = 0;

        let mut consecutive_errors = 0u32;
        const MAX_CONSECUTIVE_ERRORS: u32 = 5;

        while let Some(msg) = self.stream.next().await {
            let msg = match msg {
                Ok(m) => {
                    consecutive_errors = 0; // Reset on success
                    m
                }
                Err(e) => {
                    consecutive_errors += 1;
                    error!(
                        error = %e,
                        consecutive_errors,
                        "Error receiving message"
                    );
                    // After too many consecutive errors, return to trigger reconnection
                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        return Err(CdcError::Nats(format!(
                            "Too many consecutive errors ({}), reconnection needed: {}",
                            consecutive_errors, e
                        )));
                    }
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
            if !lsn_gte(&event.lsn, &self.snapshot_lsn) {
                skipped_lsn += 1;
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "Failed to ack message");
                }
                continue;
            }

            // Skip delete events entirely
            if event.op == "delete" {
                skipped_delete += 1;
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

            // For update events, only process if status transitioned to "active"
            if event.op == "update" {
                match market_data.status.as_deref() {
                    Some("active") => {
                        // Market transitioned to active — subscribe if not already tracked.
                        // This handles markets that were created as "initialized" before the
                        // connector started and only became active later via secmaster sync.
                        if self.subscribed_markets.contains(&market_data.ticker) {
                            debug!(
                                ticker = %market_data.ticker,
                                "CDC: Market transitioned to active (already subscribed)"
                            );
                            if let Err(e) = msg.ack().await {
                                warn!(error = %e, "Failed to ack message");
                            }
                            continue;
                        }
                        // Not yet subscribed — check category filter then subscribe
                        match self.resolve_decision(&market_data.event_ticker).await {
                            SubscribeDecision::Subscribe => {}
                            SubscribeDecision::SkipCategory => {
                                skipped_category += 1;
                                if let Err(e) = msg.ack().await {
                                    warn!(error = %e, "Failed to ack message");
                                }
                                continue;
                            }
                            SubscribeDecision::Retry => {
                                if Self::nak_or_give_up(
                                    &msg,
                                    &market_data.ticker,
                                    &market_data.event_ticker,
                                    &self.feed,
                                    &self.category,
                                )
                                .await
                                {
                                    retried += 1;
                                } else {
                                    dropped_after_retries += 1;
                                }
                                continue;
                            }
                        }
                        warn!(
                            ticker = %market_data.ticker,
                            event_ticker = %market_data.event_ticker,
                            "CDC: Market transitioned to active, sending subscribe"
                        );
                        if event_tx.send(ShardEvent::Subscribe(market_data.ticker.clone())).await.is_err() {
                            error!("Event channel closed, stopping CDC consumer");
                            break;
                        }
                        self.subscribed_markets.insert(market_data.ticker);
                        subscribed_update += 1;
                        if let Err(e) = msg.ack().await {
                            warn!(error = %e, "Failed to ack message");
                        }
                        continue;
                    }
                    Some(status) if is_terminal_status(status) => {
                        // Market settled/closed -- trigger unsubscribe if we're subscribed
                        if self.subscribed_markets.remove(&market_data.ticker) {
                            warn!(
                                ticker = %market_data.ticker,
                                status,
                                "CDC: Market became terminal, sending unsubscribe"
                            );
                            if event_tx.send(ShardEvent::Unsubscribe(market_data.ticker.clone())).await.is_err() {
                                error!("Event channel closed, stopping CDC consumer");
                                break;
                            }
                            unsubscribed += 1;
                        } else {
                            debug!(
                                ticker = %market_data.ticker,
                                status,
                                "CDC: Terminal status for untracked market, skipping"
                            );
                        }
                        if let Err(e) = msg.ack().await {
                            warn!(error = %e, "Failed to ack message");
                        }
                        continue;
                    }
                    _ => {
                        skipped_update_inactive += 1;
                        if let Err(e) = msg.ack().await {
                            warn!(error = %e, "Failed to ack message");
                        }
                        continue;
                    }
                }
            }

            // Skip already subscribed markets
            if self.subscribed_markets.contains(&market_data.ticker) {
                skipped_duplicate += 1;
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "Failed to ack message");
                }
                continue;
            }

            // Check if market's event category matches our filter
            match self.resolve_decision(&market_data.event_ticker).await {
                SubscribeDecision::Subscribe => {}
                SubscribeDecision::SkipCategory => {
                    skipped_category += 1;
                    if let Err(e) = msg.ack().await {
                        warn!(error = %e, "Failed to ack message");
                    }
                    continue;
                }
                SubscribeDecision::Retry => {
                    if Self::nak_or_give_up(
                        &msg,
                        &market_data.ticker,
                        &market_data.event_ticker,
                        &self.feed,
                        &self.category,
                    )
                    .await
                    {
                        retried += 1;
                    } else {
                        dropped_after_retries += 1;
                    }
                    continue;
                }
            }

            // Send ticker for subscription
            warn!(
                ticker = %market_data.ticker,
                event_ticker = %market_data.event_ticker,
                "CDC: New market for subscription"
            );

            if event_tx.send(ShardEvent::Subscribe(market_data.ticker.clone())).await.is_err() {
                error!("Event channel closed, stopping CDC consumer");
                break;
            }

            self.subscribed_markets.insert(market_data.ticker);
            subscribed += 1;

            // Ack the message
            if let Err(e) = msg.ack().await {
                warn!(error = %e, "Failed to ack message");
            }

            // Log progress periodically
            if processed % 100 == 0 {
                info!(
                    processed,
                    skipped_lsn,
                    skipped_category,
                    skipped_duplicate,
                    skipped_delete,
                    skipped_update_inactive,
                    subscribed,
                    subscribed_update,
                    unsubscribed,
                    retried,
                    dropped_after_retries,
                    "CDC consumer progress"
                );
            }
        }

        warn!(
            processed,
            skipped_lsn,
            skipped_category,
            skipped_duplicate,
            skipped_delete,
            skipped_update_inactive,
            subscribed,
            subscribed_update,
            unsubscribed,
            retried,
            dropped_after_retries,
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
        assert!(lsn_gte("0/16B3748", "0/16B3748"));
        assert!(lsn_gte("0/16B3749", "0/16B3748"));
        assert!(!lsn_gte("0/16B3747", "0/16B3748"));
        assert!(lsn_gte("1/0", "0/FFFFFF"));
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
        assert_eq!(market.status, None);
    }

    #[test]
    fn test_cdc_update_event_parse() {
        let json = r#"{
            "lsn": "0/16B3748",
            "table": "markets",
            "op": "update",
            "key": {"ticker": "KXTEST-123"},
            "data": {
                "ticker": "KXTEST-123",
                "event_ticker": "KXEVENT-1",
                "status": "active"
            }
        }"#;

        let event: CdcEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.op, "update");

        let market: MarketData = serde_json::from_value(event.data.unwrap()).unwrap();
        assert_eq!(market.ticker, "KXTEST-123");
        assert_eq!(market.event_ticker, "KXEVENT-1");
        assert_eq!(market.status, Some("active".to_string()));
    }

    fn cats(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn decide_subscribes_when_no_filter() {
        // Empty filter = subscribe to everything, even with unknown category.
        let empty = HashSet::new();
        assert_eq!(
            decide_from_category(None, &empty),
            SubscribeDecision::Subscribe
        );
        assert_eq!(
            decide_from_category(Some("Sports"), &empty),
            SubscribeDecision::Subscribe
        );
    }

    #[test]
    fn decide_subscribes_on_matching_category() {
        let filter = cats(&["Crypto"]);
        assert_eq!(
            decide_from_category(Some("Crypto"), &filter),
            SubscribeDecision::Subscribe
        );
    }

    #[test]
    fn decide_skips_on_nonmatching_category() {
        let filter = cats(&["Crypto"]);
        assert_eq!(
            decide_from_category(Some("Sports"), &filter),
            SubscribeDecision::SkipCategory
        );
    }

    #[test]
    fn decide_retries_when_category_unknown() {
        // The critical regression guard: a failed/absent lookup must RETRY,
        // never be treated as a permanent skip (which dropped the market).
        let filter = cats(&["Crypto"]);
        assert_eq!(
            decide_from_category(None, &filter),
            SubscribeDecision::Retry
        );
    }

    #[test]
    fn decide_matches_one_of_several_categories() {
        let filter = cats(&["Crypto", "Economics"]);
        assert_eq!(
            decide_from_category(Some("Economics"), &filter),
            SubscribeDecision::Subscribe
        );
        assert_eq!(
            decide_from_category(Some("Politics"), &filter),
            SubscribeDecision::SkipCategory
        );
    }

    #[test]
    fn series_suffix_matches_15m_markets() {
        assert!(series_matches_suffix("KXBTC15M-26JUN031400", "15M"));
        assert!(series_matches_suffix("KXETH15M-26JUN031400-15", "15M"));
        assert!(series_matches_suffix("KXNEWCOIN15M-26JUN0314", "15M"));
    }

    #[test]
    fn series_suffix_rejects_non_15m_markets() {
        // Hourly / daily crypto must not match the 15M suffix.
        assert!(!series_matches_suffix("KXBTCD-26JUN0314", "15M"));
        assert!(!series_matches_suffix("KXBTC-26JUN03", "15M"));
        assert!(!series_matches_suffix("KXETHD-26JUN0314-T2750", "15M"));
    }

    #[test]
    fn series_suffix_handles_malformed_ticker() {
        assert!(!series_matches_suffix("", "15M"));
        assert!(series_matches_suffix("KXBTC15M", "15M")); // no dash, whole string is series
    }

    #[test]
    fn retry_constants_bound_backoff_duration() {
        // Sanity: total retry window is long enough to ride out a transient
        // secmaster outage but still terminates.
        assert!(MAX_LOOKUP_RETRIES > ALERT_DELIVERY_THRESHOLD);
        assert!(LOOKUP_RETRY_BACKOFF.as_secs() >= 1);
    }

    #[test]
    fn test_terminal_status_detection() {
        assert!(is_terminal_status("determined"));
        assert!(is_terminal_status("settled"));
        assert!(is_terminal_status("closed"));
        assert!(is_terminal_status("finalized"));
        assert!(is_terminal_status("deactivated"));
        assert!(!is_terminal_status("active"));
        assert!(!is_terminal_status("inactive"));
        assert!(!is_terminal_status(""));
    }

    #[test]
    fn test_cdc_delete_event_parse() {
        let json = r#"{
            "lsn": "0/16B3748",
            "table": "markets",
            "op": "delete",
            "key": {"ticker": "KXTEST-123"},
            "data": null
        }"#;

        let event: CdcEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.op, "delete");
        assert!(event.data.is_none());
    }
}
