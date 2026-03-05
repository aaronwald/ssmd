use async_nats::jetstream::{self, consumer::pull::Stream, Context};
use futures_util::StreamExt;
use ssmd_middleware::{lsn_gte, lsn::Lsn};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};
use tokio_postgres::{Client, NoTls};
use chrono::Utc;
use crate::{Result, Error, cache::RedisCache, metrics::CacheMetrics};

/// CDC event from NATS (matches ssmd-cdc publisher format)
#[derive(Debug, serde::Deserialize)]
pub struct CdcEvent {
    pub lsn: String,
    pub table: String,
    pub op: String,  // "insert", "update", "delete"
    pub key: serde_json::Value,
    pub data: Option<serde_json::Value>,
}

/// Max entries in the event→series L1 cache.
/// PostgreSQL L2 fallback handles evicted entries.
const EVENT_SERIES_CACHE_CAP: usize = 10_000;

/// Lookup cache for event_ticker -> series_ticker mapping
/// Uses LRU cache as L1 (bounded), PostgreSQL as L2 fallback
pub struct EventSeriesLookup {
    cache: LruCache<String, String>,
    db_client: Client,
}

impl EventSeriesLookup {
    pub async fn new(database_url: &str) -> Result<Self> {
        let (client, connection) = tokio_postgres::connect(database_url, NoTls).await
            .map_err(|e| Error::Database(format!("Connection failed: {}", e)))?;

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "EventSeriesLookup DB connection error");
            }
        });

        Ok(Self {
            cache: LruCache::new(NonZeroUsize::new(EVENT_SERIES_CACHE_CAP).unwrap()),
            db_client: client,
        })
    }

    /// Get series_ticker - check in-memory first, then query PostgreSQL
    pub async fn get_series(&mut self, event_ticker: &str) -> Option<String> {
        // L1: Check in-memory cache first (fast path)
        if let Some(series) = self.cache.get(event_ticker) {
            return Some(series.clone());
        }

        // L2: Query PostgreSQL
        match self.db_client
            .query_opt(
                "SELECT series_ticker FROM events WHERE event_ticker = $1",
                &[&event_ticker],
            )
            .await
        {
            Ok(Some(row)) => {
                let series_ticker: String = row.get(0);
                // Cache for future lookups
                self.cache.put(event_ticker.to_string(), series_ticker.clone());
                Some(series_ticker)
            }
            Ok(None) => {
                tracing::debug!(event_ticker, "Event not found in database");
                None
            }
            Err(e) => {
                tracing::warn!(event_ticker, error = %e, "Failed to query series_ticker");
                None
            }
        }
    }

    /// Update cache from event CDC data (no DB query needed)
    pub fn update_from_event(&mut self, event_ticker: &str, data: &serde_json::Value) {
        if let Some(series) = data.get("series_ticker").and_then(|v| v.as_str()) {
            self.cache.put(event_ticker.to_string(), series.to_string());
        }
    }
}

pub struct CdcConsumer {
    stream: Stream,
    snapshot_lsn: String,
    event_series_lookup: EventSeriesLookup,
    metrics: CacheMetrics,
}

impl CdcConsumer {
    pub async fn new(
        nats_url: &str,
        stream_name: &str,
        consumer_name: &str,
        snapshot_lsn: String,
        database_url: &str,
        metrics: CacheMetrics,
    ) -> Result<Self> {
        let client = async_nats::connect(nats_url).await
            .map_err(|e| Error::Nats(format!("Connection failed: {}", e)))?;
        let js: Context = jetstream::new(client);

        // Get or create consumer
        let stream_obj = js.get_stream(stream_name).await
            .map_err(|e| Error::Nats(format!("Get stream failed: {}", e)))?;

        let consumer = stream_obj
            .get_or_create_consumer(
                consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(consumer_name.to_string()),
                    filter_subject: "cdc.>".to_string(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| Error::Nats(format!("Create consumer failed: {}", e)))?;

        // Set heartbeat to 5s to detect stale connections
        let messages = consumer.stream()
            .heartbeat(Duration::from_secs(5))
            .messages()
            .await
            .map_err(|e| Error::Nats(format!("Get messages failed: {}", e)))?;

        // Create event→series lookup with DB connection
        let event_series_lookup = EventSeriesLookup::new(database_url).await?;

        Ok(Self {
            stream: messages,
            snapshot_lsn,
            event_series_lookup,
            metrics,
        })
    }

    /// Process CDC events and update monitor hashes in Redis.
    /// Only writes to monitor:* hash keys — no secmaster:* individual keys.
    pub async fn run(&mut self, cache: &RedisCache) -> Result<()> {
        tracing::info!(snapshot_lsn = %self.snapshot_lsn, "Starting CDC consumer");

        let mut processed: u64 = 0;
        let mut skipped_lsn: u64 = 0;
        let mut last_lsn: Option<Lsn> = None;
        let mut last_event_time = Instant::now();
        let mut gaps_detected: u64 = 0;

        while let Some(msg) = self.stream.next().await {
            // Warn if no events for more than 1 hour (potential stall)
            let elapsed = last_event_time.elapsed();
            if elapsed > Duration::from_secs(3600) {
                tracing::warn!(
                    elapsed_secs = elapsed.as_secs(),
                    "No CDC events received for extended period (>1hr)"
                );
            }
            last_event_time = Instant::now();
            self.metrics.last_event_timestamp.set(Utc::now().timestamp() as f64);
            let msg = msg.map_err(|e| Error::Nats(format!("Message error: {}", e)))?;

            match serde_json::from_slice::<CdcEvent>(&msg.payload) {
                Ok(event) => {
                    // Skip events before snapshot LSN
                    if !lsn_gte(&event.lsn, &self.snapshot_lsn) {
                        skipped_lsn += 1;
                        self.metrics.skipped.inc();
                        msg.ack().await.map_err(|e| Error::Nats(format!("Ack failed: {}", e)))?;
                        continue;
                    }

                    // Detect gaps in LSN sequence
                    if let Some(current_lsn) = Lsn::parse(&event.lsn) {
                        if let Some(ref prev_lsn) = last_lsn {
                            // Log if LSN goes backwards (shouldn't happen normally)
                            if !current_lsn.gte(prev_lsn) {
                                tracing::warn!(
                                    current = %event.lsn,
                                    previous = ?prev_lsn,
                                    "LSN went backwards - possible reprocessing"
                                );
                                gaps_detected += 1;
                                self.metrics.gaps.inc();
                            }
                        }
                        last_lsn = Some(current_lsn);
                    }

                    // Extract key
                    let key = match &event.key {
                        serde_json::Value::Object(obj) => {
                            obj.values().next()
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        }
                        _ => None,
                    };

                    if let Some(key) = key {
                        match event.table.as_str() {
                            "markets" => {
                                self.handle_market_event(&event, &key, cache).await?;
                            }
                            "events" => {
                                self.handle_event_event(&event, &key, cache).await?;
                            }
                            "pairs" => {
                                self.handle_pairs_event(&event, &key, cache).await?;
                            }
                            "polymarket_conditions" => {
                                self.handle_polymarket_condition_event(&event, &key, cache).await?;
                            }
                            "market_lifecycle_events" => {
                                self.handle_lifecycle_event(&event, cache).await?;
                            }
                            _ => {
                                // Other tables (series, series_fees, etc.) — no cache action needed
                            }
                        }
                    }

                    processed += 1;
                    self.metrics.cdc_events.with_label_values(&[&event.table, &event.op]).inc();
                    if processed % 100 == 0 {
                        tracing::info!(
                            processed,
                            skipped_lsn,
                            gaps_detected,
                            last_lsn = ?last_lsn,
                            "CDC events processed"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to parse CDC event");
                }
            }

            msg.ack().await.map_err(|e| Error::Nats(format!("Ack failed: {}", e)))?;
        }

        Ok(())
    }

    /// Handle market CDC events — update monitor:markets:{event} hash.
    /// HSET on active status, HDEL on terminal status (settled, closed, etc.)
    async fn handle_market_event(
        &mut self,
        event: &CdcEvent,
        market_ticker: &str,
        cache: &RedisCache,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    let event_ticker = data.get("event_ticker")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
                    let hash_key = format!("monitor:markets:{}", event_ticker);

                    if status == "active" {
                        let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let close_time = data.get("close_time").and_then(|v| v.as_str());
                        let expected_expiration_time = data.get("expected_expiration_time").and_then(|v| v.as_str());
                        let val = serde_json::json!({
                            "title": title,
                            "status": status,
                            "close_time": close_time,
                            "expected_expiration_time": expected_expiration_time,
                        });
                        if let Err(e) = cache.hset(&hash_key, market_ticker, &val.to_string()).await {
                            tracing::warn!(error = %e, "Failed to update monitor:markets index");
                        } else {
                            self.metrics.redis_writes.with_label_values(&["hset"]).inc();
                        }
                    } else {
                        // Terminal status — remove from monitor hash
                        if let Err(e) = cache.hdel(&hash_key, market_ticker).await {
                            tracing::warn!(error = %e, "Failed to HDEL from monitor:markets");
                        } else {
                            self.metrics.redis_writes.with_label_values(&["hdel"]).inc();
                        }
                        tracing::debug!(market_ticker, status, "HDEL market from monitor");
                    }
                }
            }
            "delete" => {
                tracing::debug!(market_ticker, "Market delete - cannot determine event");
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle event CDC events — update monitor:events:{series} hash.
    /// HSET on active status, HDEL on terminal status.
    /// Also updates the event→series lookup cache.
    async fn handle_event_event(
        &mut self,
        event: &CdcEvent,
        event_ticker: &str,
        cache: &RedisCache,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    // Update our lookup cache
                    self.event_series_lookup.update_from_event(event_ticker, data);

                    let series_ticker = data.get("series_ticker")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");

                    let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
                    let hash_key = format!("monitor:events:{}", series_ticker);

                    if status == "active" {
                        let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let strike_date = data.get("strike_date").and_then(|v| v.as_str());
                        let val = serde_json::json!({
                            "title": title,
                            "status": status,
                            "strike_date": strike_date,
                            "market_count": 0,
                        });
                        if let Err(e) = cache.hset(&hash_key, event_ticker, &val.to_string()).await {
                            tracing::warn!(error = %e, "Failed to update monitor:events index");
                        } else {
                            self.metrics.redis_writes.with_label_values(&["hset"]).inc();
                        }
                    } else {
                        // Terminal status — remove from monitor hash
                        if let Err(e) = cache.hdel(&hash_key, event_ticker).await {
                            tracing::warn!(error = %e, "Failed to HDEL from monitor:events");
                        } else {
                            self.metrics.redis_writes.with_label_values(&["hdel"]).inc();
                        }
                        tracing::debug!(event_ticker, status, "HDEL event from monitor");
                    }
                }
            }
            "delete" => {
                tracing::debug!(event_ticker, "Event delete - cannot determine series");
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle pairs CDC events (Kraken futures) — update monitor hierarchy.
    /// HSET on active status, HDEL on terminal/deleted status.
    async fn handle_pairs_event(
        &self,
        event: &CdcEvent,
        pair_id: &str,
        cache: &RedisCache,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
                    let deleted_at = data.get("deleted_at");
                    let is_active = status == "active"
                        && (deleted_at.is_none() || deleted_at == Some(&serde_json::Value::Null));

                    let base = data.get("base").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
                    let event_key = format!("{}-perps", base);
                    let markets_hash = format!("monitor:markets:{}", event_key);

                    if is_active {
                        let market_type = data.get("market_type").and_then(|v| v.as_str()).unwrap_or("perpetual");
                        let contract_type = data.get("contract_type").and_then(|v| v.as_str());
                        let tradeable = data.get("tradeable").and_then(|v| v.as_bool());
                        let suspended = data.get("suspended").and_then(|v| v.as_bool());

                        let market_val = serde_json::json!({
                            "pair_id": pair_id,
                            "market_type": market_type,
                            "status": status,
                            "mark_price": data.get("mark_price"),
                            "funding_rate": data.get("funding_rate"),
                            "open_interest": data.get("open_interest"),
                            "contract_type": contract_type,
                            "tradeable": tradeable,
                            "suspended": suspended,
                            "exchange": "kraken-futures",
                            "price_type": "asset_price",
                        });
                        if let Err(e) = cache.hset(&markets_hash, pair_id, &market_val.to_string()).await {
                            tracing::warn!(error = %e, "Failed to update monitor:markets for Kraken pair");
                        } else {
                            self.metrics.redis_writes.with_label_values(&["hset"]).inc();
                        }
                    } else {
                        // Not active — remove from monitor hash
                        if let Err(e) = cache.hdel(&markets_hash, pair_id).await {
                            tracing::warn!(error = %e, "Failed to HDEL Kraken pair from monitor");
                        } else {
                            self.metrics.redis_writes.with_label_values(&["hdel"]).inc();
                        }
                        tracing::debug!(pair_id, status, "HDEL Kraken pair from monitor");
                    }
                }
            }
            "delete" => {
                tracing::debug!(pair_id, "Pair delete - periodic refresh will clean up");
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle polymarket_conditions CDC events — update monitor hierarchy.
    /// HSET on active status, HDEL on terminal/deleted status.
    async fn handle_polymarket_condition_event(
        &self,
        event: &CdcEvent,
        condition_id: &str,
        cache: &RedisCache,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
                    let deleted_at = data.get("deleted_at");
                    let is_active = status == "active"
                        && (deleted_at.is_none() || deleted_at == Some(&serde_json::Value::Null));

                    let category = data.get("category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Uncategorized");
                    let series_key = format!("PM:{}", category);
                    let events_hash = format!("monitor:events:{}", series_key);

                    if is_active {
                        let question = data.get("question").and_then(|v| v.as_str()).unwrap_or("");
                        let end_date = data.get("end_date").and_then(|v| v.as_str());
                        let accepting_orders = data.get("accepting_orders").and_then(|v| v.as_bool());
                        let event_id = data.get("event_id").and_then(|v| v.as_str());

                        let event_val = serde_json::json!({
                            "title": question,
                            "status": status,
                            "end_date": end_date,
                            "accepting_orders": accepting_orders,
                            "event_id": event_id,
                            "exchange": "polymarket",
                            "price_type": "probability",
                        });
                        if let Err(e) = cache.hset(&events_hash, condition_id, &event_val.to_string()).await {
                            tracing::warn!(error = %e, "Failed to update monitor:events for Polymarket condition");
                        } else {
                            self.metrics.redis_writes.with_label_values(&["hset"]).inc();
                        }
                    } else {
                        // Not active — remove from monitor hash
                        if let Err(e) = cache.hdel(&events_hash, condition_id).await {
                            tracing::warn!(error = %e, "Failed to HDEL Polymarket condition from monitor");
                        } else {
                            self.metrics.redis_writes.with_label_values(&["hdel"]).inc();
                        }
                        tracing::debug!(condition_id, status, "HDEL Polymarket condition from monitor");
                    }
                }
            }
            "delete" => {
                tracing::debug!(condition_id, "Polymarket condition delete - periodic refresh will clean up");
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle market_lifecycle_events CDC events — append lifecycle entries to
    /// the existing market JSON in monitor:markets:{event} hash.
    /// Only processes inserts (lifecycle events are append-only).
    async fn handle_lifecycle_event(
        &self,
        event: &CdcEvent,
        cache: &RedisCache,
    ) -> Result<()> {
        if event.op.as_str() != "insert" {
            return Ok(());
        }

        let data = match &event.data {
            Some(d) => d,
            None => return Ok(()),
        };

        let market_ticker = match data.get("market_ticker").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => {
                tracing::warn!("Lifecycle event missing market_ticker");
                return Ok(());
            }
        };

        let event_type = data.get("event_type").and_then(|v| v.as_str()).unwrap_or("unknown");
        let received_at = data.get("received_at").and_then(|v| v.as_str());
        let metadata = data.get("metadata");

        // Derive event_ticker from market_ticker (first two '-' segments)
        let event_ticker = extract_event_ticker(market_ticker);
        let hash_key = format!("monitor:markets:{}", event_ticker);

        // Read existing market JSON from Redis
        let existing = match cache.hget(&hash_key, market_ticker).await {
            Ok(Some(json_str)) => json_str,
            Ok(None) => {
                // Market not in cache yet — warmer will populate later
                tracing::debug!(market_ticker, "Lifecycle event for uncached market, skipping");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(market_ticker, error = %e, "Failed to HGET market for lifecycle append");
                return Ok(());
            }
        };

        // Parse existing JSON and append lifecycle event
        let mut market_json: serde_json::Value = match serde_json::from_str(&existing) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(market_ticker, error = %e, "Failed to parse market JSON");
                return Ok(());
            }
        };

        let lifecycle_entry = serde_json::json!({
            "type": event_type,
            "ts": received_at,
            "metadata": metadata,
        });

        // Get or create the lifecycle_events array
        let lifecycle_events = market_json
            .as_object_mut()
            .and_then(|obj| {
                if !obj.contains_key("lifecycle_events") {
                    obj.insert("lifecycle_events".to_string(), serde_json::json!([]));
                }
                obj.get_mut("lifecycle_events")
            })
            .and_then(|v| v.as_array_mut());

        if let Some(arr) = lifecycle_events {
            arr.push(lifecycle_entry);
        }

        // Write back
        if let Err(e) = cache.hset(&hash_key, market_ticker, &market_json.to_string()).await {
            tracing::warn!(market_ticker, error = %e, "Failed to write lifecycle event to cache");
        } else {
            self.metrics.redis_writes.with_label_values(&["hset"]).inc();
            tracing::debug!(market_ticker, event_type, "Appended lifecycle event to cache");
        }

        Ok(())
    }
}

/// Extract event_ticker from market_ticker.
/// Market tickers use '-' segments: the event_ticker is the first two segments.
/// e.g. "KXNBAGAME-26MAR05BOSLAL-BOS" -> "KXNBAGAME-26MAR05BOSLAL"
/// e.g. "KXBTCD-26MAR0211-T5060" -> "KXBTCD-26MAR0211"
/// Single-segment tickers (no dash) return the full string.
fn extract_event_ticker(market_ticker: &str) -> &str {
    let mut dash_count = 0;
    for (i, c) in market_ticker.char_indices() {
        if c == '-' {
            dash_count += 1;
            if dash_count == 2 {
                return &market_ticker[..i];
            }
        }
    }
    // 0 or 1 dashes — the whole ticker is the event ticker
    market_ticker
}
