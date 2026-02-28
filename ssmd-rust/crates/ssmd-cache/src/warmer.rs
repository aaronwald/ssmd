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

    /// Warm markets table into Redis (only live markets — active + not yet expired)
    /// Key structure: secmaster:series:SERIES:event:EVENT:market:MARKET
    pub async fn warm_markets(&self, cache: &RedisCache) -> Result<u64> {
        // Join markets with events to get series_ticker and event_ticker
        // Only cache live markets (active + close_time in future)
        let rows = self.client
            .query(
                r#"
                SELECT
                    m.ticker,
                    m.event_ticker,
                    e.series_ticker,
                    row_to_json(m.*)
                FROM markets m
                JOIN events e ON m.event_ticker = e.event_ticker
                WHERE m.status = 'active'
                  AND (m.close_time IS NULL OR m.close_time > NOW())
                "#,
                &[],
            )
            .await?;

        let mut count = 0;
        for row in rows {
            let market_ticker: String = row.get(0);
            let event_ticker: String = row.get(1);
            let series_ticker: String = row.get(2);
            let json: serde_json::Value = row.get(3);

            if cache.set_market(&series_ticker, &event_ticker, &market_ticker, &json).await? {
                count += 1;
            }
        }

        tracing::info!(count, "Warmed active markets");
        Ok(count)
    }

    /// Warm events table into Redis (only live events — active + not yet expired)
    /// Key structure: secmaster:series:SERIES:event:EVENT
    pub async fn warm_events(&self, cache: &RedisCache) -> Result<u64> {
        let rows = self.client
            .query(
                "SELECT event_ticker, series_ticker, row_to_json(events.*) FROM events WHERE status = 'active' AND (strike_date IS NULL OR strike_date > NOW())",
                &[],
            )
            .await?;

        let mut count = 0;
        for row in rows {
            let event_ticker: String = row.get(0);
            let series_ticker: String = row.get(1);
            let json: serde_json::Value = row.get(2);
            if cache.set_event(&series_ticker, &event_ticker, &json).await? {
                count += 1;
            }
        }

        tracing::info!(count, "Warmed active events");
        Ok(count)
    }

    /// Warm series table into Redis
    /// Key structure: secmaster:series:SERIES_TICKER
    pub async fn warm_series(&self, cache: &RedisCache) -> Result<u64> {
        let rows = self.client
            .query("SELECT ticker, row_to_json(series.*) FROM series", &[])
            .await?;

        let mut count = 0;
        for row in rows {
            let ticker: String = row.get(0);
            let json: serde_json::Value = row.get(1);
            cache.set_series(&ticker, &json).await?;
            count += 1;
        }

        tracing::info!(count, "Warmed series");
        Ok(count)
    }

    /// Warm series_fees table into Redis
    pub async fn warm_fees(&self, cache: &RedisCache) -> Result<u64> {
        let rows = self.client
            .query("SELECT series_ticker, row_to_json(series_fees.*) FROM series_fees", &[])
            .await?;

        let mut count = 0;
        for row in rows {
            let series_ticker: String = row.get(0);
            let json: serde_json::Value = row.get(1);
            cache.set("fee", &series_ticker, &json).await?;
            count += 1;
        }

        tracing::info!(count, "Warmed fees");
        Ok(count)
    }

    /// Build monitor index hashes from Postgres aggregations.
    /// Creates:
    ///   monitor:categories          → { cat: {"event_count":N,"series_count":N} }
    ///   monitor:series:{category}   → { series: {"title":"...","active_events":N,"active_markets":N} }
    ///   monitor:events:{series}     → { event: {"title":"...","status":"...","strike_date":"...","market_count":N} }
    ///   monitor:markets:{event}     → { market: {"title":"...","status":"...","close_time":"..."} }
    pub async fn warm_monitor_indexes(&self, cache: &RedisCache) -> Result<u64> {
        let start = std::time::Instant::now();

        // 1. Categories: aggregate from live events (not yet expired)
        let cat_rows = self.client
            .query(
                r#"
                SELECT e.category,
                       COUNT(DISTINCT e.event_ticker) AS event_count,
                       COUNT(DISTINCT e.series_ticker) AS series_count
                FROM events e
                WHERE e.status = 'active'
                  AND e.category IS NOT NULL
                  AND (e.strike_date IS NULL OR e.strike_date > NOW())
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
        tracing::info!(categories = cat_rows.len(), "Warmed monitor:categories");

        // 2. Series per category: live events + live markets counts
        let series_rows = self.client
            .query(
                r#"
                SELECT e.category, s.ticker, s.title,
                       COUNT(DISTINCT e.event_ticker) AS active_events,
                       COUNT(DISTINCT m.ticker) AS active_markets
                FROM series s
                JOIN events e ON e.series_ticker = s.ticker AND e.status = 'active'
                  AND (e.strike_date IS NULL OR e.strike_date > NOW())
                LEFT JOIN markets m ON m.event_ticker = e.event_ticker AND m.status = 'active'
                  AND (m.close_time IS NULL OR m.close_time > NOW())
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

        // 3. Events per series: only live events (strike_date in future)
        let event_rows = self.client
            .query(
                r#"
                SELECT e.series_ticker, e.event_ticker, e.title, e.status,
                       e.strike_date::text,
                       COUNT(m.ticker) AS market_count
                FROM events e
                LEFT JOIN markets m ON m.event_ticker = e.event_ticker AND m.status = 'active'
                  AND (m.close_time IS NULL OR m.close_time > NOW())
                WHERE e.status = 'active'
                  AND (e.strike_date IS NULL OR e.strike_date > NOW())
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
                JOIN events e ON m.event_ticker = e.event_ticker
                WHERE m.status = 'active' AND e.status = 'active'
                  AND (m.close_time IS NULL OR m.close_time > NOW())
                  AND (e.strike_date IS NULL OR e.strike_date > NOW())
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

        let elapsed = start.elapsed();
        tracing::info!(
            total_keys,
            elapsed_ms = elapsed.as_millis(),
            "Monitor index warming complete"
        );

        Ok(total_keys)
    }

    /// Warm all tables
    pub async fn warm_all(&self, cache: &RedisCache) -> Result<String> {
        let start = std::time::Instant::now();

        // Get LSN before warming
        let lsn = self.current_lsn().await?;
        tracing::info!(lsn = %lsn, "Snapshot LSN");

        // Warm each table (series first so markets can reference them)
        let series = self.warm_series(cache).await?;
        let markets = self.warm_markets(cache).await?;
        let events = self.warm_events(cache).await?;
        let fees = self.warm_fees(cache).await?;

        // Build monitor index hashes from the warmed data
        let indexes = self.warm_monitor_indexes(cache).await?;

        let elapsed = start.elapsed();
        tracing::info!(
            series,
            markets,
            events,
            fees,
            indexes,
            elapsed_ms = elapsed.as_millis(),
            "Cache warming complete"
        );

        Ok(lsn)
    }
}
