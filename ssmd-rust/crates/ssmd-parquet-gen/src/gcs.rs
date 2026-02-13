use anyhow::Result;
use bytes::Bytes;
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
use std::sync::Arc;

pub struct GcsClient {
    store: Arc<dyn ObjectStore>,
}

impl GcsClient {
    /// Build from environment (Workload Identity or GOOGLE_APPLICATION_CREDENTIALS)
    pub fn from_env(bucket: &str) -> Result<Self> {
        let store = GoogleCloudStorageBuilder::from_env()
            .with_bucket_name(bucket)
            .build()?;
        Ok(Self {
            store: Arc::new(store),
        })
    }

    /// List all .jsonl.gz files under a prefix
    pub async fn list_jsonl_files(&self, prefix: &str) -> Result<Vec<String>> {
        use futures_util::StreamExt;
        let prefix_path = ObjectPath::from(prefix);
        let mut paths = Vec::new();
        let mut stream = self.store.list(Some(&prefix_path));
        while let Some(meta) = stream.next().await {
            let meta = meta?;
            let path = meta.location.to_string();
            if path.ends_with(".jsonl.gz") {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }

    /// Download a file and return its bytes
    pub async fn get(&self, path: &str) -> Result<Bytes> {
        let obj_path = ObjectPath::from(path);
        let result = self.store.get(&obj_path).await?;
        Ok(result.bytes().await?)
    }

    /// Upload bytes to a path
    pub async fn put(&self, path: &str, data: Bytes) -> Result<()> {
        let obj_path = ObjectPath::from(path);
        self.store.put(&obj_path, PutPayload::from(data)).await?;
        Ok(())
    }

    /// Check if a path exists
    pub async fn exists(&self, path: &str) -> Result<bool> {
        let obj_path = ObjectPath::from(path);
        match self.store.head(&obj_path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

}
