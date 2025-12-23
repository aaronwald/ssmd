use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::StorageError;
use crate::storage::{ObjectMeta, Storage};

type BucketData = HashMap<String, (Bytes, ObjectMeta)>;
type StorageData = HashMap<String, BucketData>;

pub struct InMemoryStorage {
    data: Arc<RwLock<StorageData>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self { data: Arc::new(RwLock::new(HashMap::new())) }
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after UNIX epoch")
            .as_millis() as u64
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Storage for InMemoryStorage {
    async fn put(&self, bucket: &str, key: &str, data: Bytes) -> Result<ObjectMeta, StorageError> {
        let meta = ObjectMeta {
            key: key.to_string(),
            size: data.len() as u64,
            last_modified: Self::now_millis(),
            etag: Some(format!("{:x}", md5::compute(&data))),
            content_type: None,
        };
        let mut store = self.data.write().await;
        let bucket_data = store.entry(bucket.to_string()).or_default();
        bucket_data.insert(key.to_string(), (data, meta.clone()));
        Ok(meta)
    }

    async fn get(&self, bucket: &str, key: &str) -> Result<Bytes, StorageError> {
        let store = self.data.read().await;
        store.get(bucket).and_then(|b| b.get(key)).map(|(data, _)| data.clone())
            .ok_or_else(|| StorageError::NotFound(format!("{}/{}", bucket, key)))
    }

    async fn exists(&self, bucket: &str, key: &str) -> Result<bool, StorageError> {
        let store = self.data.read().await;
        Ok(store.get(bucket).map(|b| b.contains_key(key)).unwrap_or(false))
    }

    async fn head(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        let store = self.data.read().await;
        store.get(bucket).and_then(|b| b.get(key)).map(|(_, meta)| meta.clone())
            .ok_or_else(|| StorageError::NotFound(format!("{}/{}", bucket, key)))
    }

    async fn delete(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        let mut store = self.data.write().await;
        if let Some(bucket_data) = store.get_mut(bucket) { bucket_data.remove(key); }
        Ok(())
    }

    async fn list(&self, bucket: &str, prefix: &str) -> Result<Vec<ObjectMeta>, StorageError> {
        let store = self.data.read().await;
        Ok(store.get(bucket).map(|b| b.iter().filter(|(k, _)| k.starts_with(prefix)).map(|(_, (_, meta))| meta.clone()).collect()).unwrap_or_default())
    }

    async fn create_bucket(&self, bucket: &str) -> Result<(), StorageError> {
        let mut store = self.data.write().await;
        store.entry(bucket.to_string()).or_default();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_put_and_get() {
        let storage = InMemoryStorage::new();
        let data = Bytes::from("hello world");
        storage.put("bucket", "file.txt", data.clone()).await.unwrap();
        let retrieved = storage.get("bucket", "file.txt").await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_not_found() {
        let storage = InMemoryStorage::new();
        let result = storage.get("bucket", "missing").await;
        assert!(matches!(result, Err(StorageError::NotFound(_))));
    }
}
