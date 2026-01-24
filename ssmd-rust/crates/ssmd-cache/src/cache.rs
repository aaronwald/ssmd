use redis::AsyncCommands;
use crate::Result;

/// TTL for settled markets: 1 day in seconds
const SETTLED_TTL_SECS: i64 = 86400;

pub struct RedisCache {
    conn: redis::aio::MultiplexedConnection,
}

impl RedisCache {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let mut conn = client.get_multiplexed_async_connection().await?;

        // Test connection
        let _: String = redis::cmd("PING").query_async(&mut conn).await?;

        tracing::info!("Connected to Redis");
        Ok(Self { conn })
    }

    /// Set a secmaster record (no expiry)
    pub async fn set(&self, table: &str, key: &str, value: &serde_json::Value) -> Result<()> {
        let redis_key = format!("secmaster:{}:{}", table, key);
        let json = serde_json::to_string(value)?;

        let mut conn = self.conn.clone();
        conn.set::<_, _, ()>(&redis_key, &json).await?;

        tracing::debug!(key = %redis_key, "SET");
        Ok(())
    }

    /// Set a secmaster record with absolute expiry time (Unix timestamp)
    pub async fn set_with_expiry(
        &self,
        redis_key: &str,
        value: &serde_json::Value,
        expire_at: i64,
    ) -> Result<()> {
        let json = serde_json::to_string(value)?;

        let mut conn = self.conn.clone();
        // SET then EXPIREAT in a pipeline
        redis::pipe()
            .set(redis_key, &json)
            .expire_at(redis_key, expire_at)
            .query_async::<()>(&mut conn)
            .await?;

        tracing::debug!(key = %redis_key, expire_at, "SET with EXPIREAT");
        Ok(())
    }

    /// Set series metadata
    /// Key: secmaster:series:SERIES_TICKER
    pub async fn set_series(&self, ticker: &str, data: &serde_json::Value) -> Result<()> {
        let redis_key = format!("secmaster:series:{}", ticker);
        let json = serde_json::to_string(data)?;

        let mut conn = self.conn.clone();
        conn.set::<_, _, ()>(&redis_key, &json).await?;

        tracing::debug!(key = %redis_key, "SET series");
        Ok(())
    }

    /// Set a market record under series/event hierarchy with status-aware TTL
    /// Key: secmaster:series:SERIES:EVENT:MARKET
    /// - Active/closed markets: no expiry
    /// - Settled markets: expire 1 day after close_time (or now if no close_time)
    /// - Already expired settled markets (>1 day old): not cached
    pub async fn set_market(
        &self,
        series_ticker: &str,
        event_ticker: &str,
        market_ticker: &str,
        data: &serde_json::Value,
    ) -> Result<bool> {
        let redis_key = format!(
            "secmaster:series:{}:{}:{}",
            series_ticker, event_ticker, market_ticker
        );
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");

        if status == "settled" {
            let now = chrono::Utc::now().timestamp();

            // Parse close_time if available
            let close_time = data
                .get("close_time")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.timestamp());

            let expire_at = match close_time {
                Some(ct) => ct + SETTLED_TTL_SECS,
                None => now + SETTLED_TTL_SECS, // Fallback: expire 1 day from now
            };

            // Skip if already expired
            if expire_at <= now {
                tracing::debug!(
                    market_ticker,
                    series_ticker,
                    expire_at,
                    now,
                    "Skipping expired settled market"
                );
                return Ok(false);
            }

            self.set_with_expiry(&redis_key, data, expire_at).await?;
        } else {
            // Active/closed markets: no expiry
            let json = serde_json::to_string(data)?;
            let mut conn = self.conn.clone();
            conn.set::<_, _, ()>(&redis_key, &json).await?;
            tracing::debug!(key = %redis_key, "SET market");
        }

        Ok(true)
    }

    /// Set an event record under series hierarchy with status-aware TTL
    /// Key: secmaster:series:SERIES:EVENT
    /// - Active events: no expiry
    /// - Non-active events (settled, closed, etc.): expire 1 day after strike_date
    /// - Already expired events (>1 day old): not cached
    pub async fn set_event(
        &self,
        series_ticker: &str,
        event_ticker: &str,
        data: &serde_json::Value,
    ) -> Result<bool> {
        let redis_key = format!("secmaster:series:{}:{}", series_ticker, event_ticker);
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");

        // Treat non-active status as terminal states (settled, closed, finalized, etc.)
        if status != "active" {
            let now = chrono::Utc::now().timestamp();

            // Parse strike_date if available
            let strike_date = data
                .get("strike_date")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.timestamp());

            let expire_at = match strike_date {
                Some(sd) => sd + SETTLED_TTL_SECS,
                None => now + SETTLED_TTL_SECS, // Fallback: expire 1 day from now
            };

            // Skip if already expired
            if expire_at <= now {
                tracing::debug!(
                    event_ticker,
                    expire_at,
                    now,
                    "Skipping expired event"
                );
                return Ok(false);
            }

            self.set_with_expiry(&redis_key, data, expire_at).await?;
        } else {
            // Active events: no expiry
            let json = serde_json::to_string(data)?;
            let mut conn = self.conn.clone();
            conn.set::<_, _, ()>(&redis_key, &json).await?;
            tracing::debug!(key = %redis_key, "SET event");
        }

        Ok(true)
    }

    /// Delete a secmaster record
    pub async fn delete(&self, table: &str, key: &str) -> Result<()> {
        let redis_key = format!("secmaster:{}:{}", table, key);

        let mut conn = self.conn.clone();
        conn.del::<_, ()>(&redis_key).await?;

        tracing::debug!(key = %redis_key, "DEL");
        Ok(())
    }

    /// Delete an event under series hierarchy
    pub async fn delete_event(&self, series_ticker: &str, event_ticker: &str) -> Result<()> {
        let redis_key = format!("secmaster:series:{}:{}", series_ticker, event_ticker);

        let mut conn = self.conn.clone();
        conn.del::<_, ()>(&redis_key).await?;

        tracing::debug!(key = %redis_key, "DEL event");
        Ok(())
    }

    /// Delete a market under series/event hierarchy
    pub async fn delete_market(&self, series_ticker: &str, event_ticker: &str, market_ticker: &str) -> Result<()> {
        let redis_key = format!(
            "secmaster:series:{}:{}:{}",
            series_ticker, event_ticker, market_ticker
        );

        let mut conn = self.conn.clone();
        conn.del::<_, ()>(&redis_key).await?;

        tracing::debug!(key = %redis_key, "DEL market");
        Ok(())
    }

    /// Get count of keys matching pattern
    pub async fn count(&self, pattern: &str) -> Result<u64> {
        let mut conn = self.conn.clone();
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(pattern)
            .query_async(&mut conn)
            .await?;

        Ok(keys.len() as u64)
    }

    /// Get count of series
    pub async fn count_series(&self) -> Result<u64> {
        // Match series but not their markets
        self.count("secmaster:series:*").await.map(|n| {
            // This counts both series and markets, we need to subtract markets
            // For now, just return the pattern match - we'll refine if needed
            n
        })
    }
}
