use tokio_postgres::{Client, NoTls};
use crate::{Result, cache::RedisCache};

pub struct CacheWarmer {
    client: Client,
}

impl CacheWarmer {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "PostgreSQL connection error");
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
    /// Uses DEL-before-repopulate: clears all monitor:* keys first, then rebuilds from
    /// Postgres with `WHERE status = 'active'`. This provides a 5-minute bound on stale
    /// data even if CDC misses a lifecycle transition.
    ///
    /// Creates:
    ///   monitor:categories          → { cat: {"event_count":N,"series_count":N} }
    ///   monitor:series:{category}   → { series: {"title":"...","active_events":N,"active_markets":N} }
    ///   monitor:events:{series}     → { event: {"title":"...","status":"...","strike_date":"...","market_count":N} }
    ///   monitor:markets:{event}     → { market: {"title":"...","status":"...","close_time":"..."} }
    ///
    /// Also warms Kraken pairs and Polymarket conditions into the same hierarchy.
    pub async fn warm_monitor_indexes(&self, cache: &RedisCache) -> Result<u64> {
        let start = std::time::Instant::now();

        // DEL all monitor:* keys before repopulating — clean slate every refresh
        let deleted = cache.del_pattern("monitor:*").await?;
        tracing::info!(deleted, "Cleared stale monitor keys");

        // 1. Categories: only categories that have events with live markets
        let cat_rows = self.client
            .query(
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
                &[],
            )
            .await?;

        let mut total_keys: u64 = 0;
        for row in &cat_rows {
            let category: String = row.get(0);
            let event_count: i64 = row.get(1);
            let series_count: i64 = row.get(2);
            let val = serde_json::json!({
                "event_count": event_count,
                "series_count": series_count,
            });
            cache.hset("monitor:categories", &category, &val.to_string()).await?;
        }
        total_keys += cat_rows.len() as u64;
        tracing::info!(categories = cat_rows.len(), "Warmed monitor:categories (Kalshi)");

        // 2. Series per category: only series with live events/markets
        let series_rows = self.client
            .query(
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
                &[],
            )
            .await?;

        for row in &series_rows {
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
            let hash_key = format!("monitor:series:{}", category);
            cache.hset(&hash_key, &ticker, &val.to_string()).await?;
        }
        total_keys += series_rows.len() as u64;
        tracing::info!(series_entries = series_rows.len(), "Warmed monitor:series:*");

        // 3. Events per series: only events with live markets, with accurate market_count
        let event_rows = self.client
            .query(
                r#"
                SELECT e.series_ticker, e.event_ticker, e.title, e.status,
                       e.strike_date::text,
                       COUNT(m.ticker) AS market_count
                FROM events e
                JOIN markets m ON m.event_ticker = e.event_ticker
                  AND m.status = 'active' AND m.close_time > NOW()
                WHERE e.status = 'active'
                GROUP BY e.series_ticker, e.event_ticker, e.title, e.status, e.strike_date
                "#,
                &[],
            )
            .await?;

        for row in &event_rows {
            let series_ticker: String = row.get(0);
            let event_ticker: String = row.get(1);
            let title: Option<String> = row.get(2);
            let status: String = row.get(3);
            let strike_date: Option<String> = row.get(4);
            let market_count: i64 = row.get(5);
            let val = serde_json::json!({
                "title": title.unwrap_or_default(),
                "status": status,
                "strike_date": strike_date,
                "market_count": market_count,
            });
            let hash_key = format!("monitor:events:{}", series_ticker);
            cache.hset(&hash_key, &event_ticker, &val.to_string()).await?;
        }
        total_keys += event_rows.len() as u64;
        tracing::info!(event_entries = event_rows.len(), "Warmed monitor:events:*");

        // 4. Markets per event: only live markets (close_time in future)
        let market_rows = self.client
            .query(
                r#"
                SELECT m.event_ticker, m.ticker, m.title, m.status, m.close_time::text
                FROM markets m
                WHERE m.status = 'active'
                  AND m.close_time > NOW()
                "#,
                &[],
            )
            .await?;

        for row in &market_rows {
            let event_ticker: String = row.get(0);
            let market_ticker: String = row.get(1);
            let title: Option<String> = row.get(2);
            let status: String = row.get(3);
            let close_time: Option<String> = row.get(4);
            let val = serde_json::json!({
                "title": title.unwrap_or_default(),
                "status": status,
                "close_time": close_time,
            });
            let hash_key = format!("monitor:markets:{}", event_ticker);
            cache.hset(&hash_key, &market_ticker, &val.to_string()).await?;
        }
        total_keys += market_rows.len() as u64;
        tracing::info!(market_entries = market_rows.len(), "Warmed monitor:markets:*");

        // 5. Kraken Futures pairs → merged into monitor hierarchy
        total_keys += self.warm_pairs_monitor(cache).await?;

        // 6. Polymarket conditions/tokens → merged into monitor hierarchy
        total_keys += self.warm_polymarket_monitor(cache).await?;

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
    /// Hierarchy mapping:
    ///   Category: "Kraken Futures"
    ///   Series:   base currency group (BTC, ETH, etc.)
    ///   Event:    "{base}-perps" synthetic event for perpetuals
    ///   Market:   "kraken:{pair_id}" (e.g., "kraken:PF_XBTUSD")
    async fn warm_pairs_monitor(&self, cache: &RedisCache) -> Result<u64> {
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

        // Group by base currency for series/event aggregation
        let mut base_groups: std::collections::HashMap<String, Vec<&tokio_postgres::Row>> =
            std::collections::HashMap::new();
        for row in &rows {
            let base: String = row.get(1);
            base_groups.entry(base).or_default().push(row);
        }

        let mut total_keys: u64 = 0;

        // Category: "Kraken Futures"
        let cat_val = serde_json::json!({
            "instrument_count": rows.len(),
            "base_count": base_groups.len(),
        });
        cache.hset("monitor:categories", "Kraken Futures", &cat_val.to_string()).await?;
        total_keys += 1;

        // Series + Events + Markets per base currency
        for (base, group) in &base_groups {
            // Series: base currency under "Kraken Futures" category
            let active_pairs: usize = group.len();
            let series_val = serde_json::json!({
                "title": format!("{} Perpetuals", base),
                "active_pairs": active_pairs,
            });
            cache.hset("monitor:series:Kraken Futures", base, &series_val.to_string()).await?;
            total_keys += 1;

            // Event: synthetic "{base}-perps"
            let event_key = format!("{}-perps", base);
            let event_val = serde_json::json!({
                "title": format!("Active {} Perps", base),
                "pair_count": active_pairs,
            });
            let events_hash = format!("monitor:events:{}", base);
            cache.hset(&events_hash, &event_key, &event_val.to_string()).await?;
            total_keys += 1;

            // Markets: each pair under the synthetic event
            let markets_hash = format!("monitor:markets:{}", event_key);
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

                let market_key = pair_id.clone();
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
                cache.hset(&markets_hash, &market_key, &market_val.to_string()).await?;
                total_keys += 1;
            }
        }

        tracing::info!(
            pairs = rows.len(),
            base_groups = base_groups.len(),
            "Warmed Kraken pairs into monitor hierarchy"
        );

        Ok(total_keys)
    }

    /// Warm Polymarket conditions and tokens into the monitor hierarchy.
    ///
    /// Hierarchy mapping:
    ///   Category: condition.category (merged with Kalshi categories where overlapping)
    ///   Series:   "PM:{category}" synthetic series
    ///   Event:    condition_id
    ///   Market:   token_id
    async fn warm_polymarket_monitor(&self, cache: &RedisCache) -> Result<u64> {
        // Query active conditions with their tokens
        let condition_rows = self.client
            .query(
                r#"
                SELECT c.condition_id, c.question, c.category, c.status,
                       c.end_date::text, c.accepting_orders, c.event_id,
                       COUNT(t.token_id) AS token_count
                FROM polymarket_conditions c
                LEFT JOIN polymarket_tokens t ON t.condition_id = c.condition_id
                WHERE c.deleted_at IS NULL
                  AND c.status = 'active'
                GROUP BY c.condition_id, c.question, c.category, c.status,
                         c.end_date, c.accepting_orders, c.event_id
                "#,
                &[],
            )
            .await?;

        if condition_rows.is_empty() {
            tracing::info!("No active Polymarket conditions to warm");
            return Ok(0);
        }

        // Group by category for aggregation
        let mut cat_groups: std::collections::HashMap<String, Vec<&tokio_postgres::Row>> =
            std::collections::HashMap::new();
        for row in &condition_rows {
            let category: Option<String> = row.get(2);
            let cat = category.unwrap_or_else(|| "Uncategorized".to_string());
            cat_groups.entry(cat).or_default().push(row);
        }

        let mut total_keys: u64 = 0;

        // Categories: merge PM condition counts into monitor:categories
        // Read-merge-write to preserve existing Kalshi fields (event_count, series_count)
        for (category, group) in &cat_groups {
            let existing = cache.hget("monitor:categories", category).await?;
            let mut val: serde_json::Value = existing
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            val["pm_condition_count"] = serde_json::json!(group.len());
            cache.hset("monitor:categories", category, &val.to_string()).await?;
            total_keys += 1;

            // Series: "PM:{category}" under each category
            let series_key = format!("PM:{}", category);
            let series_val = serde_json::json!({
                "title": format!("Polymarket {}", category),
                "active_conditions": group.len(),
            });
            let series_hash = format!("monitor:series:{}", category);
            cache.hset(&series_hash, &series_key, &series_val.to_string()).await?;
            total_keys += 1;

            // Events: each condition is an event under the PM series
            let events_hash = format!("monitor:events:{}", series_key);
            for row in group {
                let condition_id: String = row.get(0);
                let question: String = row.get(1);
                let status: String = row.get(3);
                let end_date: Option<String> = row.get(4);
                let accepting_orders: Option<bool> = row.get(5);
                let event_id: Option<String> = row.get(6);
                let token_count: i64 = row.get(7);

                let event_val = serde_json::json!({
                    "title": question,
                    "status": status,
                    "end_date": end_date,
                    "accepting_orders": accepting_orders,
                    "event_id": event_id,
                    "token_count": token_count,
                    "exchange": "polymarket",
                    "price_type": "probability",
                });
                cache.hset(&events_hash, &condition_id, &event_val.to_string()).await?;
                total_keys += 1;
            }
        }

        // Markets: tokens under each condition
        let token_rows = self.client
            .query(
                r#"
                SELECT t.token_id, t.condition_id, t.outcome, t.outcome_index,
                       t.price::text
                FROM polymarket_tokens t
                JOIN polymarket_conditions c ON c.condition_id = t.condition_id
                WHERE c.deleted_at IS NULL
                  AND c.status = 'active'
                "#,
                &[],
            )
            .await?;

        for row in &token_rows {
            let token_id: String = row.get(0);
            let condition_id: String = row.get(1);
            let outcome: String = row.get(2);
            let outcome_index: i32 = row.get(3);
            let price: Option<String> = row.get(4);

            let market_val = serde_json::json!({
                "outcome": outcome,
                "outcome_index": outcome_index,
                "price": price,
                "exchange": "polymarket",
                "price_type": "probability",
            });
            let markets_hash = format!("monitor:markets:{}", condition_id);
            cache.hset(&markets_hash, &token_id, &market_val.to_string()).await?;
            total_keys += 1;
        }

        tracing::info!(
            conditions = condition_rows.len(),
            tokens = token_rows.len(),
            categories = cat_groups.len(),
            "Warmed Polymarket conditions into monitor hierarchy"
        );

        Ok(total_keys)
    }

    /// Warm cache on startup — only monitor indexes (the tradable universe).
    pub async fn warm_all(&self, cache: &RedisCache) -> Result<String> {
        let start = std::time::Instant::now();

        // Get LSN before warming
        let lsn = self.current_lsn().await?;
        tracing::info!(lsn = %lsn, "Snapshot LSN");

        // Build monitor index hashes (the only data consumers actually read)
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
