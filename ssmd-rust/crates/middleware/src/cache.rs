use async_trait::async_trait;
use bytes::Bytes;
use std::time::Duration;

use crate::error::CacheError;

/// Cache abstraction for fast key-value lookups
#[async_trait]
pub trait Cache: Send + Sync {
    /// Get a value
    async fn get(&self, key: &str) -> Result<Option<Bytes>, CacheError>;

    /// Set a value with optional TTL
    async fn set(&self, key: &str, value: Bytes, ttl: Option<Duration>) -> Result<(), CacheError>;

    /// Delete a key
    async fn delete(&self, key: &str) -> Result<(), CacheError>;

    /// Check existence
    async fn exists(&self, key: &str) -> Result<bool, CacheError>;

    /// Set if not exists (for locking)
    async fn set_nx(
        &self,
        key: &str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> Result<bool, CacheError>;

    /// Get multiple keys
    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<Bytes>>, CacheError>;

    /// Set multiple keys
    async fn mset(&self, pairs: &[(&str, Bytes)]) -> Result<(), CacheError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_trait_compiles() {
        // Trait definition test - just verifies it compiles
        fn _assert_send_sync<T: Send + Sync>() {}
        fn _assert_cache<T: Cache>() {
            _assert_send_sync::<T>();
        }
    }
}
