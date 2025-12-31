use redis::AsyncCommands;
use crate::Result;

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

    /// Set a secmaster record
    pub async fn set(&self, table: &str, key: &str, value: &serde_json::Value) -> Result<()> {
        let redis_key = format!("secmaster:{}:{}", table, key);
        let json = serde_json::to_string(value)?;

        let mut conn = self.conn.clone();
        conn.set::<_, _, ()>(&redis_key, &json).await?;

        tracing::debug!(key = %redis_key, "SET");
        Ok(())
    }

    /// Delete a secmaster record
    pub async fn delete(&self, table: &str, key: &str) -> Result<()> {
        let redis_key = format!("secmaster:{}:{}", table, key);

        let mut conn = self.conn.clone();
        conn.del::<_, ()>(&redis_key).await?;

        tracing::debug!(key = %redis_key, "DEL");
        Ok(())
    }

    /// Get count of secmaster keys
    pub async fn count(&self, table: &str) -> Result<u64> {
        let pattern = format!("secmaster:{}:*", table);

        let mut conn = self.conn.clone();
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await?;

        Ok(keys.len() as u64)
    }
}
