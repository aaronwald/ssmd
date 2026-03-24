use futures_util::TryStreamExt;
use tokio_postgres::{Client, NoTls};
use crate::{Result, Error, cache::RedisCache};

pub struct CacheWarmer {
    client: Client,
}

impl CacheWarmer {
    pub async fn connect(database_url: &str) -> Result<Self> {
        if database_url.is_empty() {
            return Err(Error::Database("DATABASE_URL is empty".to_string()));
        }
        let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "PostgreSQL connection error — exiting");
                std::process::exit(1);
            }
        });

        Ok(Self { client })
    }

    /// Get current WAL LSN (for race condition handling)
    pub async fn current_lsn(&self) -> Result<String> {
        let row = self.client
            .query_one("SELECT pg_current_wal_lsn()::text", &[])
            .await?;
        Ok(row.get(0))
    }

    /// Build monitor index hashes for hierarchical browsing.
    /// Only includes live data: events with at least one market whose close_time > NOW().
    ///
    /// Uses :_tmp suffix keys with atomic RENAME to avoid empty reads during rebuild.
    /// All queries use streaming cursors to avoid materializing full result sets in memory.
    ///
    /// Creates:
    ///   monitor:categories          → { cat: {"event_count":N,"series_count":N} }
    ///   monitor:series:{category}   → { series: {"title":"...","active_events":N,"active_markets":N} }
    ///   monitor:events:{series}     → { event: {"title":"...","status":"...","strike_date":"...","market_count":N} }
    ///   monitor:markets:{event}     → { market: {"title":"...","status":"...","close_time":"..."} }
    ///
    /// Also warms Kraken pairs into the same hierarchy.
    pub async fn warm_monitor_indexes(&self, cache: &RedisCache) -> Result<u64> {
        let start = std::time::Instant::now();

        // Write all data to :_tmp suffix keys, then atomically RENAME to final keys.
        let mut tmp_keys: Vec<String> = Vec::new();
        let mut final_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut total_keys: u64 = 0;

        // 1. Categories: only categories that have events with live markets
        let tmp_cat_key = "monitor:categories:_tmp".to_string();
        {
            let stream = self.client
                .query_raw(
                    r#"
                    SELECT e.category,
                           COUNT(DISTINCT e.event_ticker) AS event_count,
                           COUNT(DISTINCT e.series_ticker) AS series_count
                    FROM events e
                    WHERE e.status = 'active'
                      AND e.category IS NOT NULL
                      AND EXISTS (
                        SELECT 1 FROM markets m
                        WHERE m.event_ticker = e.event_ticker
                          AND m.status = 'active'
                          AND m.close_time > NOW()
                      )
                    GROUP BY e.category
                    "#,
                    &[] as &[&str],
                )
                .await?;
            tokio::pin!(stream);
            let mut count = 0u64;
            while let Some(row) = stream.try_next().await? {
                let category: String = row.get(0);
                let event_count: i64 = row.get(1);
                let series_count: i64 = row.get(2);
                let val = serde_json::json!({
                    "event_count": event_count,
                    "series_count": series_count,
                });
                cache.hset(&tmp_cat_key, &category, &val.to_string()).await?;
                count += 1;
            }
            tmp_keys.push(tmp_cat_key);
            final_keys.insert("monitor:categories".to_string());
            total_keys += count;
            tracing::info!(categories = count, "Warmed monitor:categories:_tmp");
        }

        // 2. Series per category: only series with live events/markets
        {
            let stream = self.client
                .query_raw(
                    r#"
                    SELECT e.category, s.ticker, s.title,
                           COUNT(DISTINCT e.event_ticker) AS active_events,
                           COUNT(DISTINCT m.ticker) AS active_markets
                    FROM series s
                    JOIN events e ON e.series_ticker = s.ticker AND e.status = 'active'
                      AND EXISTS (
                        SELECT 1 FROM markets m2
                        WHERE m2.event_ticker = e.event_ticker
                          AND m2.status = 'active'
                          AND m2.close_time > NOW()
                      )
                    LEFT JOIN markets m ON m.event_ticker = e.event_ticker
                      AND m.status = 'active' AND m.close_time > NOW()
                    WHERE e.category IS NOT NULL
                    GROUP BY e.category, s.ticker, s.title
                    "#,
                    &[] as &[&str],
                )
                .await?;
            tokio::pin!(stream);
            let mut count = 0u64;
            while let Some(row) = stream.try_next().await? {
                let category: String = row.get(0);
                let ticker: String = row.get(1);
                let title: Option<String> = row.get(2);
                let active_events: i64 = row.get(3);
                let active_markets: i64 = row.get(4);
                let val = serde_json::json!({
                    "title": title.unwrap_or_default(),
                    "active_events": active_events,
                    "active_markets": active_markets,
                });
                let tmp_key = format!("monitor:series:{}:_tmp", category);
                let final_key = format!("monitor:series:{}", category);
                cache.hset(&tmp_key, &ticker, &val.to_string()).await?;
                if final_keys.insert(final_key) {
                    tmp_keys.push(tmp_key);
                }
                count += 1;
            }
            total_keys += count;
            tracing::info!(series_entries = count, "Warmed monitor:series:*:_tmp");
        }

        // 3. Events per series: only events with live markets, with accurate market_count
        {
            let stream = self.client
                .query_raw(
                    r#"
                    SELECT e.series_ticker, e.event_ticker, e.title, e.status,
                           e.strike_date::text,
                           COUNT(m.ticker) AS market_count,
                           MIN(m.expected_expiration_time)::text AS expected_expiration_time
                    FROM events e
                    JOIN markets m ON m.event_ticker = e.event_ticker
                      AND m.status = 'active' AND m.close_time > NOW()
                    WHERE e.status = 'active'
                    GROUP BY e.series_ticker, e.event_ticker, e.title, e.status, e.strike_date
                    "#,
                    &[] as &[&str],
                )
                .await?;
            tokio::pin!(stream);
            let mut count = 0u64;
            while let Some(row) = stream.try_next().await? {
                let series_ticker: String = row.get(0);
                let event_ticker: String = row.get(1);
                let title: Option<String> = row.get(2);
                let status: String = row.get(3);
                let strike_date: Option<String> = row.get(4);
                let market_count: i64 = row.get(5);
                let expected_expiration_time: Option<String> = row.get(6);
                let val = serde_json::json!({
                    "title": title.unwrap_or_default(),
                    "status": status,
                    "strike_date": strike_date,
                    "market_count": market_count,
                    "expected_expiration_time": expected_expiration_time,
                });
                let tmp_key = format!("monitor:events:{}:_tmp", series_ticker);
                let final_key = format!("monitor:events:{}", series_ticker);
                cache.hset(&tmp_key, &event_ticker, &val.to_string()).await?;
                if final_keys.insert(final_key) {
                    tmp_keys.push(tmp_key);
                }
                count += 1;
            }
            total_keys += count;
            tracing::info!(event_entries = count, "Warmed monitor:events:*:_tmp");
        }

        // 4. Markets per event: only live markets (close_time in future)
        {
            let stream = self.client
                .query_raw(
                    r#"
                    SELECT m.event_ticker, m.ticker, m.title, m.status, m.close_time::text,
                           m.expected_expiration_time::text
                    FROM markets m
                    WHERE m.status = 'active'
                      AND m.close_time > NOW()
                    "#,
                    &[] as &[&str],
                )
                .await?;
            tokio::pin!(stream);
            let mut count = 0u64;
            while let Some(row) = stream.try_next().await? {
                let event_ticker: String = row.get(0);
                let market_ticker: String = row.get(1);
                let title: Option<String> = row.get(2);
                let status: String = row.get(3);
                let close_time: Option<String> = row.get(4);
                let expected_expiration_time: Option<String> = row.get(5);
                let val = serde_json::json!({
                    "title": title.unwrap_or_default(),
                    "status": status,
                    "close_time": close_time,
                    "expected_expiration_time": expected_expiration_time,
                });
                let tmp_key = format!("monitor:markets:{}:_tmp", event_ticker);
                let final_key = format!("monitor:markets:{}", event_ticker);
                cache.hset(&tmp_key, &market_ticker, &val.to_string()).await?;
                if final_keys.insert(final_key) {
                    tmp_keys.push(tmp_key);
                }
                count += 1;
            }
            total_keys += count;
            tracing::info!(market_entries = count, "Warmed monitor:markets:*:_tmp");
        }

        // 4b. Warm lifecycle events into existing market hashes (streamed, grouped by market)
        {
            let stream = self.client
                .query_raw(
                    r#"
                    SELECT mle.market_ticker, mle.event_type, mle.received_at::text,
                           mle.metadata::text
                    FROM market_lifecycle_events mle
                    JOIN markets m ON m.ticker = mle.market_ticker
                    WHERE m.status = 'active'
                      AND m.close_time > NOW()
                    ORDER BY mle.market_ticker, mle.received_at
                    "#,
                    &[] as &[&str],
                )
                .await?;
            tokio::pin!(stream);

            // Stream rows and flush per-market batch when market_ticker changes.
            // ORDER BY market_ticker guarantees contiguous grouping.
            let mut current_market: Option<String> = None;
            let mut lifecycle_batch: Vec<serde_json::Value> = Vec::new();
            let mut lifecycle_count = 0u64;
            let mut total_lifecycle_events = 0u64;

            while let Some(row) = stream.try_next().await? {
                let market_ticker: String = row.get(0);
                let event_type: String = row.get(1);
                let received_at: Option<String> = row.get(2);
                let metadata_str: Option<String> = row.get(3);
                let metadata: serde_json::Value = match metadata_str {
                    Some(s) => serde_json::from_str(&s).map_err(|e| {
                        Error::Database(format!("malformed lifecycle metadata for {}: {}", market_ticker, e))
                    })?,
                    None => serde_json::json!({}),
                };
                total_lifecycle_events += 1;

                // Flush previous batch if market changed
                if current_market.as_deref() != Some(&market_ticker) {
                    if let Some(ref prev_market) = current_market {
                        lifecycle_count += flush_lifecycle(cache, prev_market, &lifecycle_batch).await?;
                    }
                    lifecycle_batch.clear();
                    current_market = Some(market_ticker.clone());
                }

                lifecycle_batch.push(serde_json::json!({
                    "type": event_type,
                    "ts": received_at.unwrap_or_default(),
                    "metadata": metadata,
                }));
            }
            // Flush final batch
            if let Some(ref prev_market) = current_market {
                lifecycle_count += flush_lifecycle(cache, prev_market, &lifecycle_batch).await?;
            }

            tracing::info!(
                markets_with_lifecycle = lifecycle_count,
                total_lifecycle_events,
                "Warmed lifecycle events into monitor:markets:*"
            );
        }

        // 5. Kraken Futures pairs → merged into monitor hierarchy
        total_keys += self.warm_pairs_monitor(cache, &mut tmp_keys, &mut final_keys).await?;

        // Atomic swap: RENAME each :_tmp key to its final name.
        let mut renamed = 0u64;
        let mut empty_deleted = 0u64;
        for tmp_key in &tmp_keys {
            let final_key = tmp_key.trim_end_matches(":_tmp");
            let len = cache.hlen(tmp_key).await?;
            if len == 0 {
                let _ = cache.del_key(tmp_key).await;
                let _ = cache.del_key(final_key).await;
                empty_deleted += 1;
            } else {
                cache.rename_key(tmp_key, final_key).await?;
                renamed += 1;
            }
        }
        tracing::info!(renamed, empty_deleted, "Atomic RENAME :_tmp → final");

        // Clean up stale monitor keys that weren't rebuilt
        let existing_keys = cache.keys("monitor:*").await?;
        let stale_keys: Vec<String> = existing_keys
            .into_iter()
            .filter(|k| !k.ends_with(":_tmp") && !final_keys.contains(k))
            .collect();
        if !stale_keys.is_empty() {
            let stale_count = cache.del_keys(&stale_keys).await?;
            tracing::info!(stale_count, "Deleted stale monitor keys");
        }

        let elapsed = start.elapsed();
        tracing::info!(
            total_keys,
            elapsed_ms = elapsed.as_millis(),
            "Monitor index warming complete"
        );

        Ok(total_keys)
    }

    /// Warm Kraken pairs into the monitor hierarchy.
    ///
    /// Kraken pairs are few (~20-50), so grouping by base currency in memory is fine.
    ///
    /// Hierarchy mapping:
    ///   Category: "Kraken Futures"
    ///   Series:   base currency group (BTC, ETH, etc.)
    ///   Event:    "{base}-perps" synthetic event for perpetuals
    ///   Market:   pair_id (e.g., "PF_XBTUSD")
    async fn warm_pairs_monitor(&self, cache: &RedisCache, tmp_keys: &mut Vec<String>, final_keys: &mut std::collections::HashSet<String>) -> Result<u64> {
        // Kraken pairs are few (~20-50), collect to group by base currency
        let rows = self.client
            .query(
                r#"
                SELECT pair_id, base, quote, market_type, status,
                       mark_price::text, funding_rate::text, open_interest::text,
                       contract_type, tradeable, suspended, ws_name
                FROM pairs
                WHERE deleted_at IS NULL
                  AND status = 'active'
                  AND exchange = 'kraken'
                "#,
                &[],
            )
            .await?;

        if rows.is_empty() {
            tracing::info!("No active Kraken pairs to warm");
            return Ok(0);
        }

        let mut base_groups: std::collections::HashMap<String, Vec<&tokio_postgres::Row>> =
            std::collections::HashMap::new();
        for row in &rows {
            let base: String = row.get(1);
            base_groups.entry(base).or_default().push(row);
        }

        let mut total_keys: u64 = 0;

        let cat_val = serde_json::json!({
            "instrument_count": rows.len(),
            "base_count": base_groups.len(),
        });
        cache.hset("monitor:categories:_tmp", "Kraken Futures", &cat_val.to_string()).await?;
        total_keys += 1;

        for (base, group) in &base_groups {
            let active_pairs: usize = group.len();
            let series_val = serde_json::json!({
                "title": format!("{} Perpetuals", base),
                "active_pairs": active_pairs,
            });
            let tmp_series = "monitor:series:Kraken Futures:_tmp".to_string();
            let final_series = "monitor:series:Kraken Futures".to_string();
            cache.hset(&tmp_series, base, &series_val.to_string()).await?;
            if final_keys.insert(final_series) {
                tmp_keys.push(tmp_series);
            }
            total_keys += 1;

            let event_key = format!("{}-perps", base);
            let event_val = serde_json::json!({
                "title": format!("Active {} Perps", base),
                "pair_count": active_pairs,
            });
            let tmp_events = format!("monitor:events:{}:_tmp", base);
            let final_events = format!("monitor:events:{}", base);
            cache.hset(&tmp_events, &event_key, &event_val.to_string()).await?;
            if final_keys.insert(final_events) {
                tmp_keys.push(tmp_events);
            }
            total_keys += 1;

            let tmp_markets = format!("monitor:markets:{}:_tmp", event_key);
            let final_markets = format!("monitor:markets:{}", event_key);
            for row in group {
                let pair_id: String = row.get(0);
                let market_type: String = row.get(3);
                let status: String = row.get(4);
                let mark_price: Option<String> = row.get(5);
                let funding_rate: Option<String> = row.get(6);
                let open_interest: Option<String> = row.get(7);
                let contract_type: Option<String> = row.get(8);
                let tradeable: Option<bool> = row.get(9);
                let suspended: Option<bool> = row.get(10);

                let market_val = serde_json::json!({
                    "pair_id": pair_id,
                    "market_type": market_type,
                    "status": status,
                    "mark_price": mark_price,
                    "funding_rate": funding_rate,
                    "open_interest": open_interest,
                    "contract_type": contract_type,
                    "tradeable": tradeable,
                    "suspended": suspended,
                    "exchange": "kraken-futures",
                    "price_type": "asset_price",
                });
                cache.hset(&tmp_markets, &pair_id, &market_val.to_string()).await?;
                total_keys += 1;
            }
            if final_keys.insert(final_markets) {
                tmp_keys.push(format!("monitor:markets:{}:_tmp", event_key));
            }
        }

        tracing::info!(
            pairs = rows.len(),
            base_groups = base_groups.len(),
            "Warmed Kraken pairs into monitor hierarchy"
        );

        Ok(total_keys)
    }

    /// Warm cache on startup — only monitor indexes (the tradable universe).
    pub async fn warm_all(&self, cache: &RedisCache) -> Result<String> {
        let start = std::time::Instant::now();

        let lsn = self.current_lsn().await?;
        tracing::info!(lsn = %lsn, "Snapshot LSN");

        let indexes = self.warm_monitor_indexes(cache).await?;

        let elapsed = start.elapsed();
        tracing::info!(
            indexes,
            elapsed_ms = elapsed.as_millis(),
            "Cache warming complete"
        );

        Ok(lsn)
    }
}

/// Flush a batch of lifecycle events into the existing market hash entry in Redis.
async fn flush_lifecycle(
    cache: &RedisCache,
    market_ticker: &str,
    events: &[serde_json::Value],
) -> Result<u64> {
    if events.is_empty() {
        return Ok(0);
    }
    let event_ticker = extract_event_ticker(market_ticker);
    let tmp_key = format!("monitor:markets:{}:_tmp", event_ticker);

    let existing: Option<String> = cache.hget(&tmp_key, market_ticker).await.unwrap_or(None);
    if let Some(existing_str) = existing {
        if let Ok(mut market_json) = serde_json::from_str::<serde_json::Value>(&existing_str) {
            if let Some(obj) = market_json.as_object_mut() {
                obj.insert("lifecycle_events".to_string(), serde_json::json!(events));
            }
            cache.hset(&tmp_key, market_ticker, &market_json.to_string()).await?;
            return Ok(1);
        }
    }
    Ok(0)
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
    market_ticker
}
