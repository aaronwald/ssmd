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

    /// Warm markets table into Redis (only active markets, grouped under series)
    /// Key structure: secmaster:series:SERIES_TICKER:market:MARKET_TICKER
    pub async fn warm_markets(&self, cache: &RedisCache) -> Result<u64> {
        // Join markets with events to get series_ticker
        // Only cache active markets during warming
        let rows = self.client
            .query(
                r#"
                SELECT
                    m.ticker,
                    e.series_ticker,
                    row_to_json(m.*)
                FROM markets m
                JOIN events e ON m.event_ticker = e.event_ticker
                WHERE m.status = 'active'
                "#,
                &[],
            )
            .await?;

        let mut count = 0;
        for row in rows {
            let market_ticker: String = row.get(0);
            let series_ticker: String = row.get(1);
            let json: serde_json::Value = row.get(2);

            if cache.set_market(&series_ticker, &market_ticker, &json).await? {
                count += 1;
            }
        }

        tracing::info!(count, "Warmed active markets");
        Ok(count)
    }

    /// Warm events table into Redis (only active events during warming)
    pub async fn warm_events(&self, cache: &RedisCache) -> Result<u64> {
        let rows = self.client
            .query(
                "SELECT event_ticker, row_to_json(events.*) FROM events WHERE status = 'active'",
                &[],
            )
            .await?;

        let mut count = 0;
        for row in rows {
            let event_ticker: String = row.get(0);
            let json: serde_json::Value = row.get(1);
            if cache.set_event(&event_ticker, &json).await? {
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

        let elapsed = start.elapsed();
        tracing::info!(
            series,
            markets,
            events,
            fees,
            elapsed_ms = elapsed.as_millis(),
            "Cache warming complete"
        );

        Ok(lsn)
    }
}
