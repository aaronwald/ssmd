use async_nats::jetstream::{self, consumer::pull::Stream, Context};
use futures_util::StreamExt;
use ssmd_middleware::lsn_gte;
use std::collections::HashMap;
use std::time::Duration;
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

/// Lookup cache for event_ticker -> series_ticker mapping
pub struct EventSeriesLookup {
    cache: HashMap<String, String>,
}

impl EventSeriesLookup {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Get series_ticker for an event, caching the result
    pub fn get_or_insert(&mut self, event_ticker: &str, data: &serde_json::Value) -> Option<String> {
        if let Some(series) = self.cache.get(event_ticker) {
            return Some(series.clone());
        }

        // Extract series_ticker from event data
        if let Some(series) = data.get("series_ticker").and_then(|v| v.as_str()) {
            self.cache.insert(event_ticker.to_string(), series.to_string());
            return Some(series.to_string());
        }

        None
    }

    /// Update cache from event CDC data
    pub fn update_from_event(&mut self, event_ticker: &str, data: &serde_json::Value) {
        if let Some(series) = data.get("series_ticker").and_then(|v| v.as_str()) {
            self.cache.insert(event_ticker.to_string(), series.to_string());
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

        Ok(Self {
            stream: messages,
            snapshot_lsn,
            event_series_lookup: EventSeriesLookup::new(),
        })
    }

    /// Process CDC events and update cache
    pub async fn run(&mut self, cache: &RedisCache) -> Result<()> {
        tracing::info!(snapshot_lsn = %self.snapshot_lsn, "Starting CDC consumer");

        let mut processed: u64 = 0;
        let mut skipped_lsn: u64 = 0;
        let mut skipped_expired: u64 = 0;

        while let Some(msg) = self.stream.next().await {
            let msg = msg.map_err(|e| Error::Nats(format!("Message error: {}", e)))?;

            match serde_json::from_slice::<CdcEvent>(&msg.payload) {
                Ok(event) => {
                    // Skip events before snapshot LSN
                    if !lsn_gte(&event.lsn, &self.snapshot_lsn) {
                        skipped_lsn += 1;
                        msg.ack().await.map_err(|e| Error::Nats(format!("Ack failed: {}", e)))?;
                        continue;
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
                    if processed.is_multiple_of(100) {
                        tracing::info!(
                            processed,
                            skipped_lsn,
                            skipped_expired,
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

                    // Look up series_ticker
                    if let Some(series_ticker) = self.event_series_lookup.get_or_insert(event_ticker, data) {
                        if !cache.set_market(&series_ticker, market_ticker, data).await? {
                            *skipped_expired += 1;
                        }
                    } else {
                        // Fallback: store under "unknown" series if lookup fails
                        tracing::warn!(
                            market_ticker,
                            event_ticker,
                            "Series lookup failed, storing under 'unknown'"
                        );
                        if !cache.set_market("unknown", market_ticker, data).await? {
                            *skipped_expired += 1;
                        }
                    }
                }
            }
            "delete" => {
                // For delete, we need the series_ticker but don't have it in the event
                // The safest approach is to delete from all possible locations
                // In practice, we could track this in the lookup cache
                tracing::debug!(market_ticker, "Market delete - cannot determine series");
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle event CDC events and update series lookup
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
                    // Store event data with TTL logic
                    if !cache.set_event(event_ticker, data).await? {
                        *skipped_expired += 1;
                    }
                }
            }
            "delete" => {
                cache.delete("event", event_ticker).await?;
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
}
