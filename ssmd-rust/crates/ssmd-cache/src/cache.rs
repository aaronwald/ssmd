use redis::AsyncCommands;
use crate::Result;

#[derive(Clone)]
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

    // --- Hash-based monitor index methods ---

    /// HGET a single field from a hash key
    pub async fn hget(&self, hash_key: &str, field: &str) -> Result<Option<String>> {
        let mut conn = self.conn.clone();
        let result: Option<String> = conn.hget(hash_key, field).await?;
        Ok(result)
    }

    /// HSET a field in a hash key
    pub async fn hset(&self, hash_key: &str, field: &str, value: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        conn.hset::<_, _, _, ()>(hash_key, field, value).await?;
        tracing::debug!(key = %hash_key, field, "HSET");
        Ok(())
    }

    /// HGETALL — return all fields and values from a hash
    pub async fn hgetall(&self, hash_key: &str) -> Result<std::collections::HashMap<String, String>> {
        let mut conn = self.conn.clone();
        let result: std::collections::HashMap<String, String> = conn.hgetall(hash_key).await?;
        tracing::debug!(key = %hash_key, fields = result.len(), "HGETALL");
        Ok(result)
    }

    /// HDEL a field from a hash key
    pub async fn hdel(&self, hash_key: &str, field: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        conn.hdel::<_, _, ()>(hash_key, field).await?;
        tracing::debug!(key = %hash_key, field, "HDEL");
        Ok(())
    }

    /// SET a string key (no TTL — refreshed by periodic warmer)
    pub async fn set_string(&self, key: &str, value: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        conn.set::<_, _, ()>(key, value).await?;
        tracing::debug!(key, len = value.len(), "SET");
        Ok(())
    }

    /// DEL an entire key (hash or string)
    pub async fn del_key(&self, key: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        conn.del::<_, ()>(key).await?;
        tracing::debug!(key, "DEL key");
        Ok(())
    }

    /// DEL all keys matching a pattern.
    /// Uses KEYS command — safe for small keyspaces (monitor:* has ~200 keys).
    pub async fn del_pattern(&self, pattern: &str) -> Result<u64> {
        let mut conn = self.conn.clone();
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(pattern)
            .query_async(&mut conn)
            .await?;

        if keys.is_empty() {
            return Ok(0);
        }

        let count = keys.len() as u64;
        let mut pipe = redis::pipe();
        for key in &keys {
            pipe.del(key);
        }
        pipe.query_async::<()>(&mut conn).await?;

        tracing::info!(pattern, count, "DEL pattern");
        Ok(count)
    }
}
