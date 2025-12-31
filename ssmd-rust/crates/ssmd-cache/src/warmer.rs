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

    /// Warm markets table into Redis
    pub async fn warm_markets(&self, cache: &RedisCache) -> Result<u64> {
        let rows = self.client
            .query("SELECT ticker, row_to_json(markets.*) FROM markets", &[])
            .await?;

        let mut count = 0;
        for row in rows {
            let ticker: String = row.get(0);
            let json: serde_json::Value = row.get(1);
            cache.set("market", &ticker, &json).await?;
            count += 1;
        }

        tracing::info!(count, "Warmed markets");
        Ok(count)
    }

    /// Warm events table into Redis
    pub async fn warm_events(&self, cache: &RedisCache) -> Result<u64> {
        let rows = self.client
            .query("SELECT event_ticker, row_to_json(events.*) FROM events", &[])
            .await?;

        let mut count = 0;
        for row in rows {
            let event_ticker: String = row.get(0);
            let json: serde_json::Value = row.get(1);
            cache.set("event", &event_ticker, &json).await?;
            count += 1;
        }

        tracing::info!(count, "Warmed events");
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

        // Warm each table
        let markets = self.warm_markets(cache).await?;
        let events = self.warm_events(cache).await?;
        let fees = self.warm_fees(cache).await?;

        let elapsed = start.elapsed();
        tracing::info!(
            markets,
            events,
            fees,
            elapsed_ms = elapsed.as_millis(),
            "Cache warming complete"
        );

        Ok(lsn)
    }
}
