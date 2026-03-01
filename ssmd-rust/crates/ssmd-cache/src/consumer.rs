use async_nats::jetstream::{self, consumer::pull::Stream, Context};
use futures_util::StreamExt;
use ssmd_middleware::{lsn_gte, lsn::Lsn};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};
use tokio_postgres::{Client, NoTls};
use crate::{Result, Error, cache::RedisCache};

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
}

impl CdcConsumer {
    pub async fn new(
        nats_url: &str,
        stream_name: &str,
        consumer_name: &str,
        snapshot_lsn: String,
        database_url: &str,
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
        })
    }

    /// Process CDC events and update cache
    pub async fn run(&mut self, cache: &RedisCache) -> Result<()> {
        tracing::info!(snapshot_lsn = %self.snapshot_lsn, "Starting CDC consumer");

        let mut processed: u64 = 0;
        let mut skipped_lsn: u64 = 0;
        let mut skipped_expired: u64 = 0;
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
            let msg = msg.map_err(|e| Error::Nats(format!("Message error: {}", e)))?;

            match serde_json::from_slice::<CdcEvent>(&msg.payload) {
                Ok(event) => {
                    // Skip events before snapshot LSN
                    if !lsn_gte(&event.lsn, &self.snapshot_lsn) {
                        skipped_lsn += 1;
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
                                self.handle_market_event(&event, &key, cache, &mut skipped_expired).await?;
                            }
                            "events" => {
                                self.handle_event_event(&event, &key, cache, &mut skipped_expired).await?;
                            }
                            "series" => {
                                self.handle_series_event(&event, &key, cache).await?;
                            }
                            "series_fees" => {
                                self.handle_fee_event(&event, &key, cache).await?;
                            }
                            "pairs" => {
                                self.handle_pairs_event(&event, &key, cache).await?;
                            }
                            "polymarket_conditions" => {
                                self.handle_polymarket_condition_event(&event, &key, cache).await?;
                            }
                            _ => {
                                // Unknown table, use generic handler
                                match event.op.as_str() {
                                    "insert" | "update" => {
                                        if let Some(data) = &event.data {
                                            cache.set(&event.table, &key, data).await?;
                                        }
                                    }
                                    "delete" => {
                                        cache.delete(&event.table, &key).await?;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    processed += 1;
                    if processed % 100 == 0 {
                        tracing::info!(
                            processed,
                            skipped_lsn,
                            skipped_expired,
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

    /// Handle market CDC events with series grouping and TTL
    /// Uses L1 (in-memory) then L2 (PostgreSQL) lookup for event→series mapping
    /// Also updates monitor:markets:{event} hash index
    async fn handle_market_event(
        &mut self,
        event: &CdcEvent,
        market_ticker: &str,
        cache: &RedisCache,
        skipped_expired: &mut u64,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    // Get event_ticker from market data
                    let event_ticker = data.get("event_ticker")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    // Get series_ticker (L1: in-memory, L2: PostgreSQL)
                    let series_ticker = self.event_series_lookup.get_series(event_ticker).await;

                    if let Some(series_ticker) = series_ticker {
                        if !cache.set_market(&series_ticker, event_ticker, market_ticker, data).await? {
                            *skipped_expired += 1;
                        }
                    } else {
                        // Fallback: store under "unknown" series if event not found
                        tracing::warn!(
                            market_ticker,
                            event_ticker,
                            "Series lookup failed (event not in DB), storing under 'unknown'"
                        );
                        if !cache.set_market("unknown", event_ticker, market_ticker, data).await? {
                            *skipped_expired += 1;
                        }
                    }

                    // Update monitor:markets:{event} hash index
                    let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
                    if status == "active" {
                        let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let close_time = data.get("close_time").and_then(|v| v.as_str());
                        let val = serde_json::json!({
                            "title": title,
                            "status": status,
                            "close_time": close_time,
                        });
                        let hash_key = format!("monitor:markets:{}", event_ticker);
                        if let Err(e) = cache.hset(&hash_key, market_ticker, &val.to_string()).await {
                            tracing::warn!(error = %e, "Failed to update monitor:markets index");
                        }
                    }
                }
            }
            "delete" => {
                // For delete, we need the series_ticker and event_ticker but don't have them in the event
                // The safest approach is to delete from all possible locations
                // In practice, we could track this in the lookup cache
                tracing::debug!(market_ticker, "Market delete - cannot determine series/event");
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle event CDC events and update series lookup
    /// Also updates monitor:events:{series} hash index
    async fn handle_event_event(
        &mut self,
        event: &CdcEvent,
        event_ticker: &str,
        cache: &RedisCache,
        skipped_expired: &mut u64,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    // Update our lookup cache
                    self.event_series_lookup.update_from_event(event_ticker, data);

                    // Get series_ticker from event data
                    let series_ticker = data.get("series_ticker")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");

                    // Store event data with TTL logic
                    if !cache.set_event(series_ticker, event_ticker, data).await? {
                        *skipped_expired += 1;
                    }

                    // Update monitor:events:{series} hash index
                    let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
                    if status == "active" {
                        let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let strike_date = data.get("strike_date").and_then(|v| v.as_str());
                        // Count markets for this event (we don't have it in CDC data, use 0 as placeholder)
                        let val = serde_json::json!({
                            "title": title,
                            "status": status,
                            "strike_date": strike_date,
                            "market_count": 0,
                        });
                        let hash_key = format!("monitor:events:{}", series_ticker);
                        if let Err(e) = cache.hset(&hash_key, event_ticker, &val.to_string()).await {
                            tracing::warn!(error = %e, "Failed to update monitor:events index");
                        }
                    }
                }
            }
            "delete" => {
                // For delete, we don't have the series_ticker
                // Would need to track this in a lookup cache
                tracing::debug!(event_ticker, "Event delete - cannot determine series");
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle series CDC events
    async fn handle_series_event(
        &self,
        event: &CdcEvent,
        series_ticker: &str,
        cache: &RedisCache,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    cache.set_series(series_ticker, data).await?;
                }
            }
            "delete" => {
                cache.delete("series", series_ticker).await?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle series_fees CDC events
    async fn handle_fee_event(
        &self,
        event: &CdcEvent,
        series_ticker: &str,
        cache: &RedisCache,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    cache.set("fee", series_ticker, data).await?;
                }
            }
            "delete" => {
                cache.delete("fee", series_ticker).await?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle pairs CDC events (Kraken futures)
    /// Updates secmaster:pair:{pair_id} and monitor hierarchy
    async fn handle_pairs_event(
        &self,
        event: &CdcEvent,
        pair_id: &str,
        cache: &RedisCache,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    // Update secmaster record
                    cache.set("pair", pair_id, data).await?;

                    // Update monitor hierarchy if active
                    let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
                    let deleted_at = data.get("deleted_at");
                    let is_active = status == "active"
                        && (deleted_at.is_none() || deleted_at == Some(&serde_json::Value::Null));

                    if is_active {
                        let base = data.get("base").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
                        let market_type = data.get("market_type").and_then(|v| v.as_str()).unwrap_or("perpetual");
                        let contract_type = data.get("contract_type").and_then(|v| v.as_str());
                        let tradeable = data.get("tradeable").and_then(|v| v.as_bool());
                        let suspended = data.get("suspended").and_then(|v| v.as_bool());

                        let market_key = pair_id.clone();
                        let event_key = format!("{}-perps", base);
                        let markets_hash = format!("monitor:markets:{}", event_key);

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
                        if let Err(e) = cache.hset(&markets_hash, &market_key, &market_val.to_string()).await {
                            tracing::warn!(error = %e, "Failed to update monitor:markets for Kraken pair");
                        }
                    }
                }
            }
            "delete" => {
                cache.delete("pair", pair_id).await?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle polymarket_conditions CDC events
    /// Updates secmaster:polymarket_condition:{condition_id} and monitor hierarchy
    async fn handle_polymarket_condition_event(
        &self,
        event: &CdcEvent,
        condition_id: &str,
        cache: &RedisCache,
    ) -> Result<()> {
        match event.op.as_str() {
            "insert" | "update" => {
                if let Some(data) = &event.data {
                    // Update secmaster record
                    cache.set("polymarket_condition", condition_id, data).await?;

                    // Update monitor hierarchy if active
                    let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
                    let deleted_at = data.get("deleted_at");
                    let is_active = status == "active"
                        && (deleted_at.is_none() || deleted_at == Some(&serde_json::Value::Null));

                    if is_active {
                        let category = data.get("category")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Uncategorized");
                        let question = data.get("question").and_then(|v| v.as_str()).unwrap_or("");
                        let end_date = data.get("end_date").and_then(|v| v.as_str());
                        let accepting_orders = data.get("accepting_orders").and_then(|v| v.as_bool());
                        let event_id = data.get("event_id").and_then(|v| v.as_str());

                        let series_key = format!("PM:{}", category);
                        let events_hash = format!("monitor:events:{}", series_key);

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
                        }
                    }
                }
            }
            "delete" => {
                cache.delete("polymarket_condition", condition_id).await?;
            }
            _ => {}
        }

        Ok(())
    }
}
