use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::cache::Cache;
use crate::error::CacheError;

struct CacheEntry {
    value: Bytes,
    expires_at: Option<Instant>,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.expires_at.map(|e| Instant::now() > e).unwrap_or(false)
    }
}

pub struct InMemoryCache {
    data: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl InMemoryCache {
    pub fn new() -> Self {
        Self { data: Arc::new(RwLock::new(HashMap::new())) }
    }
}

impl Default for InMemoryCache {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Cache for InMemoryCache {
    async fn get(&self, key: &str) -> Result<Option<Bytes>, CacheError> {
        let data = self.data.read().await;
        Ok(data.get(key).and_then(|e| if e.is_expired() { None } else { Some(e.value.clone()) }))
    }

    async fn set(&self, key: &str, value: Bytes, ttl: Option<Duration>) -> Result<(), CacheError> {
        let expires_at = ttl.map(|d| Instant::now() + d);
        let mut data = self.data.write().await;
        data.insert(key.to_string(), CacheEntry { value, expires_at });
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let mut data = self.data.write().await;
        data.remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, CacheError> {
        let data = self.data.read().await;
        Ok(data.get(key).map(|e| !e.is_expired()).unwrap_or(false))
    }

    async fn set_nx(&self, key: &str, value: Bytes, ttl: Option<Duration>) -> Result<bool, CacheError> {
        let mut data = self.data.write().await;
        if let Some(entry) = data.get(key) {
            if !entry.is_expired() { return Ok(false); }
        }
        let expires_at = ttl.map(|d| Instant::now() + d);
        data.insert(key.to_string(), CacheEntry { value, expires_at });
        Ok(true)
    }

    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<Bytes>>, CacheError> {
        let data = self.data.read().await;
        Ok(keys.iter().map(|k| data.get(*k).and_then(|e| if e.is_expired() { None } else { Some(e.value.clone()) })).collect())
    }

    async fn mset(&self, pairs: &[(&str, Bytes)]) -> Result<(), CacheError> {
        let mut data = self.data.write().await;
        for (key, value) in pairs {
            data.insert(key.to_string(), CacheEntry { value: value.clone(), expires_at: None });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_set() {
        let cache = InMemoryCache::new();
        cache.set("key", Bytes::from("value"), None).await.unwrap();
        assert_eq!(cache.get("key").await.unwrap(), Some(Bytes::from("value")));
    }

    #[tokio::test]
    async fn test_set_nx() {
        let cache = InMemoryCache::new();
        assert!(cache.set_nx("key", Bytes::from("v1"), None).await.unwrap());
        assert!(!cache.set_nx("key", Bytes::from("v2"), None).await.unwrap());
        assert_eq!(cache.get("key").await.unwrap(), Some(Bytes::from("v1")));
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        let cache = InMemoryCache::new();
        cache.set("key", Bytes::from("value"), Some(Duration::from_millis(1))).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(cache.get("key").await.unwrap().is_none());
    }
}
