use async_trait::async_trait;
use bytes::Bytes;

use crate::error::StorageError;

/// Object metadata
#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub last_modified: u64,
    pub etag: Option<String>,
    pub content_type: Option<String>,
}

/// Storage abstraction for object storage (S3, local, etc.)
#[async_trait]
pub trait Storage: Send + Sync {
    /// Put an object
    async fn put(&self, bucket: &str, key: &str, data: Bytes) -> Result<ObjectMeta, StorageError>;

    /// Get an object
    async fn get(&self, bucket: &str, key: &str) -> Result<Bytes, StorageError>;

    /// Check if object exists
    async fn exists(&self, bucket: &str, key: &str) -> Result<bool, StorageError>;

    /// Get object metadata
    async fn head(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError>;

    /// Delete an object
    async fn delete(&self, bucket: &str, key: &str) -> Result<(), StorageError>;

    /// List objects with prefix
    async fn list(&self, bucket: &str, prefix: &str) -> Result<Vec<ObjectMeta>, StorageError>;

    /// Create a bucket
    async fn create_bucket(&self, bucket: &str) -> Result<(), StorageError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_meta_creation() {
        let meta = ObjectMeta {
            key: "2025/12/23/kalshi.jsonl".to_string(),
            size: 1024,
            last_modified: 1703318400,
            etag: Some("abc123".to_string()),
            content_type: Some("application/jsonl".to_string()),
        };

        assert_eq!(meta.key, "2025/12/23/kalshi.jsonl");
        assert_eq!(meta.size, 1024);
    }
}
