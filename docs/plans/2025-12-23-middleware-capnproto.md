# Middleware & Cap'n Proto Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement middleware abstraction layer (Transport, Storage, Cache, Journal traits) and Cap'n Proto schema generation for Kalshi trade messages, preparing for actual WebSocket connection.

**Architecture:** Create a new `ssmd-middleware` crate with trait definitions and in-memory implementations for testing. Add `ssmd-schema` crate for Cap'n Proto schema compilation and generated Rust types. Integrate with existing connector crate.

**Tech Stack:** Rust, async-trait, tokio, capnp (Cap'n Proto), async-nats (future), bytes

---

## Task 1: Create ssmd-middleware Crate Skeleton

**Files:**
- Create: `ssmd-rust/crates/middleware/Cargo.toml`
- Create: `ssmd-rust/crates/middleware/src/lib.rs`
- Create: `ssmd-rust/crates/middleware/src/error.rs`
- Modify: `ssmd-rust/Cargo.toml`

**Step 1: Add crate to workspace**

In `ssmd-rust/Cargo.toml`, add middleware to members:

```toml
members = [
    "crates/metadata",
    "crates/middleware",
    "crates/connector",
    "crates/ssmd-connector",
]
```

And add new workspace dependencies:

```toml
bytes = "1"
```

**Step 2: Create Cargo.toml**

Create `ssmd-rust/crates/middleware/Cargo.toml`:

```toml
[package]
name = "ssmd-middleware"
version.workspace = true
edition.workspace = true

[dependencies]
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
bytes = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

**Step 3: Create error types**

Create `ssmd-rust/crates/middleware/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("publish failed: {0}")]
    PublishFailed(String),
    #[error("subscribe failed: {0}")]
    SubscribeFailed(String),
    #[error("timeout")]
    Timeout,
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("write failed: {0}")]
    WriteFailed(String),
    #[error("read failed: {0}")]
    ReadFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("operation failed: {0}")]
    OperationFailed(String),
}

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("append failed: {0}")]
    AppendFailed(String),
    #[error("read failed: {0}")]
    ReadFailed(String),
    #[error("topic not found: {0}")]
    TopicNotFound(String),
}
```

**Step 4: Create lib.rs**

Create `ssmd-rust/crates/middleware/src/lib.rs`:

```rust
//! ssmd-middleware: Pluggable middleware abstractions
//!
//! Provides trait-based abstractions for Transport, Storage, Cache, and Journal
//! with in-memory implementations for testing.

pub mod error;

pub use error::{CacheError, JournalError, StorageError, TransportError};
```

**Step 5: Build to verify**

Run: `cd /workspaces/ssmd && make rust-build`
Expected: Build succeeds

**Step 6: Commit**

```bash
git add ssmd-rust/Cargo.toml ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add ssmd-middleware crate skeleton"
```

---

## Task 2: Transport Trait Definition

**Files:**
- Create: `ssmd-rust/crates/middleware/src/transport.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Write the failing test**

Add to `ssmd-rust/crates/middleware/src/transport.rs`:

```rust
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::time::Duration;

use crate::error::TransportError;

/// Message envelope with metadata
#[derive(Debug, Clone)]
pub struct TransportMessage {
    pub subject: String,
    pub payload: Bytes,
    pub headers: HashMap<String, String>,
    pub timestamp: u64,
    pub sequence: Option<u64>,
}

/// Subscription handle for receiving messages
#[async_trait]
pub trait Subscription: Send + Sync {
    /// Receive next message (blocks until available)
    async fn next(&mut self) -> Result<TransportMessage, TransportError>;

    /// Acknowledge a message (for reliable delivery)
    async fn ack(&self, sequence: u64) -> Result<(), TransportError>;

    /// Unsubscribe and close
    async fn unsubscribe(self: Box<Self>) -> Result<(), TransportError>;
}

/// Transport abstraction for pub/sub messaging
#[async_trait]
pub trait Transport: Send + Sync {
    /// Publish a message (fire and forget)
    async fn publish(&self, subject: &str, payload: Bytes) -> Result<(), TransportError>;

    /// Publish with headers
    async fn publish_with_headers(
        &self,
        subject: &str,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<(), TransportError>;

    /// Subscribe to a subject pattern
    async fn subscribe(&self, subject: &str) -> Result<Box<dyn Subscription>, TransportError>;

    /// Request/reply pattern with timeout
    async fn request(
        &self,
        subject: &str,
        payload: Bytes,
        timeout: Duration,
    ) -> Result<TransportMessage, TransportError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_message_creation() {
        let msg = TransportMessage {
            subject: "kalshi.trade.BTCUSD".to_string(),
            payload: Bytes::from(r#"{"price":100}"#),
            headers: HashMap::new(),
            timestamp: 1703318400000,
            sequence: Some(1),
        };

        assert_eq!(msg.subject, "kalshi.trade.BTCUSD");
        assert_eq!(msg.sequence, Some(1));
    }
}
```

**Step 2: Update lib.rs**

Add to `ssmd-rust/crates/middleware/src/lib.rs`:

```rust
pub mod transport;

pub use transport::{Subscription, Transport, TransportMessage};
```

**Step 3: Run test to verify**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: PASS

**Step 4: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add Transport trait definition"
```

---

## Task 3: Storage Trait Definition

**Files:**
- Create: `ssmd-rust/crates/middleware/src/storage.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Write the Storage trait**

Create `ssmd-rust/crates/middleware/src/storage.rs`:

```rust
use async_trait::async_trait;
use bytes::Bytes;
use std::pin::Pin;
use futures_util::Stream;

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
```

**Step 2: Add futures-util dependency**

Update `ssmd-rust/crates/middleware/Cargo.toml`:

```toml
[dependencies]
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
bytes = { workspace = true }
tracing = { workspace = true }
futures-util = { workspace = true }
```

**Step 3: Update lib.rs**

Add to `ssmd-rust/crates/middleware/src/lib.rs`:

```rust
pub mod storage;

pub use storage::{ObjectMeta, Storage};
```

**Step 4: Run test**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: PASS

**Step 5: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add Storage trait definition"
```

---

## Task 4: Cache Trait Definition

**Files:**
- Create: `ssmd-rust/crates/middleware/src/cache.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Write the Cache trait**

Create `ssmd-rust/crates/middleware/src/cache.rs`:

```rust
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
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

    // Cache trait is defined, tests will come with InMemoryCache implementation
    #[test]
    fn test_cache_trait_compiles() {
        // Trait definition test - just verifies it compiles
        fn _assert_send_sync<T: Send + Sync>() {}
        fn _assert_cache<T: Cache>() {
            _assert_send_sync::<T>();
        }
    }
}
```

**Step 2: Update lib.rs**

Add to `ssmd-rust/crates/middleware/src/lib.rs`:

```rust
pub mod cache;

pub use cache::Cache;
```

**Step 3: Run test**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: PASS

**Step 4: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add Cache trait definition"
```

---

## Task 5: Journal Trait Definition

**Files:**
- Create: `ssmd-rust/crates/middleware/src/journal.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Write the Journal trait**

Create `ssmd-rust/crates/middleware/src/journal.rs`:

```rust
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::time::Duration;

use crate::error::JournalError;

/// Journal entry for audit trail
#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub sequence: u64,
    pub timestamp: u64,
    pub topic: String,
    pub key: Option<Bytes>,
    pub payload: Bytes,
    pub headers: HashMap<String, String>,
}

/// Position for reading from journal
#[derive(Debug, Clone)]
pub enum JournalPosition {
    /// Start from the beginning
    Beginning,
    /// Start from the end (new messages only)
    End,
    /// Start from a specific sequence number
    Sequence(u64),
    /// Start from a specific timestamp (unix millis)
    Time(u64),
}

/// Topic configuration
#[derive(Debug, Clone)]
pub struct TopicConfig {
    pub name: String,
    pub retention: Duration,
    pub compaction: bool, // Keep only latest per key
}

/// Journal reader for replay
#[async_trait]
pub trait JournalReader: Send + Sync {
    /// Read next entry (None if at end)
    async fn next(&mut self) -> Result<Option<JournalEntry>, JournalError>;

    /// Seek to a position
    async fn seek(&mut self, position: JournalPosition) -> Result<(), JournalError>;
}

/// Journal abstraction for append-only audit log
#[async_trait]
pub trait Journal: Send + Sync {
    /// Append an entry, returns sequence number
    async fn append(
        &self,
        topic: &str,
        key: Option<Bytes>,
        payload: Bytes,
    ) -> Result<u64, JournalError>;

    /// Append with headers
    async fn append_with_headers(
        &self,
        topic: &str,
        key: Option<Bytes>,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<u64, JournalError>;

    /// Create a reader from a position
    async fn reader(
        &self,
        topic: &str,
        position: JournalPosition,
    ) -> Result<Box<dyn JournalReader>, JournalError>;

    /// Get current end position (highest sequence)
    async fn end_position(&self, topic: &str) -> Result<u64, JournalError>;

    /// Create a topic
    async fn create_topic(&self, config: TopicConfig) -> Result<(), JournalError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_entry_creation() {
        let entry = JournalEntry {
            sequence: 1,
            timestamp: 1703318400000,
            topic: "ssmd.audit".to_string(),
            key: Some(Bytes::from("user:123")),
            payload: Bytes::from(r#"{"action":"login"}"#),
            headers: HashMap::new(),
        };

        assert_eq!(entry.sequence, 1);
        assert_eq!(entry.topic, "ssmd.audit");
    }

    #[test]
    fn test_journal_position_variants() {
        let _begin = JournalPosition::Beginning;
        let _end = JournalPosition::End;
        let _seq = JournalPosition::Sequence(100);
        let _time = JournalPosition::Time(1703318400000);
    }
}
```

**Step 2: Update lib.rs**

Add to `ssmd-rust/crates/middleware/src/lib.rs`:

```rust
pub mod journal;

pub use journal::{Journal, JournalEntry, JournalPosition, JournalReader, TopicConfig};
```

**Step 3: Run test**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: PASS

**Step 4: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add Journal trait definition"
```

---

## Task 6: InMemoryTransport Implementation

**Files:**
- Create: `ssmd-rust/crates/middleware/src/memory/mod.rs`
- Create: `ssmd-rust/crates/middleware/src/memory/transport.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Write the failing test**

Create `ssmd-rust/crates/middleware/src/memory/transport.rs`:

```rust
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::error::TransportError;
use crate::transport::{Subscription, Transport, TransportMessage};

/// In-memory transport for testing
pub struct InMemoryTransport {
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<TransportMessage>>>>,
    sequence: Arc<Mutex<u64>>,
}

impl InMemoryTransport {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            sequence: Arc::new(Mutex::new(0)),
        }
    }

    async fn get_or_create_channel(
        &self,
        subject: &str,
    ) -> broadcast::Sender<TransportMessage> {
        let mut channels = self.channels.write().await;
        if let Some(tx) = channels.get(subject) {
            tx.clone()
        } else {
            let (tx, _) = broadcast::channel(1024);
            channels.insert(subject.to_string(), tx.clone());
            tx
        }
    }

    async fn next_sequence(&self) -> u64 {
        let mut seq = self.sequence.lock().await;
        *seq += 1;
        *seq
    }
}

impl Default for InMemoryTransport {
    fn default() -> Self {
        Self::new()
    }
}

struct InMemorySubscription {
    rx: broadcast::Receiver<TransportMessage>,
}

#[async_trait]
impl Subscription for InMemorySubscription {
    async fn next(&mut self) -> Result<TransportMessage, TransportError> {
        self.rx
            .recv()
            .await
            .map_err(|e| TransportError::SubscribeFailed(e.to_string()))
    }

    async fn ack(&self, _sequence: u64) -> Result<(), TransportError> {
        // No-op for in-memory
        Ok(())
    }

    async fn unsubscribe(self: Box<Self>) -> Result<(), TransportError> {
        // Drop receiver
        Ok(())
    }
}

#[async_trait]
impl Transport for InMemoryTransport {
    async fn publish(&self, subject: &str, payload: Bytes) -> Result<(), TransportError> {
        self.publish_with_headers(subject, payload, HashMap::new())
            .await
    }

    async fn publish_with_headers(
        &self,
        subject: &str,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<(), TransportError> {
        let tx = self.get_or_create_channel(subject).await;
        let seq = self.next_sequence().await;
        let msg = TransportMessage {
            subject: subject.to_string(),
            payload,
            headers,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            sequence: Some(seq),
        };

        // Ignore send errors (no subscribers)
        let _ = tx.send(msg);
        Ok(())
    }

    async fn subscribe(&self, subject: &str) -> Result<Box<dyn Subscription>, TransportError> {
        let tx = self.get_or_create_channel(subject).await;
        let rx = tx.subscribe();
        Ok(Box::new(InMemorySubscription { rx }))
    }

    async fn request(
        &self,
        _subject: &str,
        _payload: Bytes,
        _timeout: Duration,
    ) -> Result<TransportMessage, TransportError> {
        // Request/reply not implemented for in-memory
        Err(TransportError::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let transport = InMemoryTransport::new();

        // Subscribe first
        let mut sub = transport.subscribe("test.subject").await.unwrap();

        // Publish a message
        transport
            .publish("test.subject", Bytes::from("hello"))
            .await
            .unwrap();

        // Receive the message
        let msg = sub.next().await.unwrap();
        assert_eq!(msg.subject, "test.subject");
        assert_eq!(msg.payload, Bytes::from("hello"));
        assert!(msg.sequence.is_some());
    }

    #[tokio::test]
    async fn test_publish_with_headers() {
        let transport = InMemoryTransport::new();
        let mut sub = transport.subscribe("test.headers").await.unwrap();

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        transport
            .publish_with_headers("test.headers", Bytes::from("{}"), headers)
            .await
            .unwrap();

        let msg = sub.next().await.unwrap();
        assert_eq!(
            msg.headers.get("content-type"),
            Some(&"application/json".to_string())
        );
    }

    #[tokio::test]
    async fn test_sequence_numbers_increment() {
        let transport = InMemoryTransport::new();
        let mut sub = transport.subscribe("test.seq").await.unwrap();

        transport.publish("test.seq", Bytes::from("1")).await.unwrap();
        transport.publish("test.seq", Bytes::from("2")).await.unwrap();

        let msg1 = sub.next().await.unwrap();
        let msg2 = sub.next().await.unwrap();

        assert_eq!(msg1.sequence, Some(1));
        assert_eq!(msg2.sequence, Some(2));
    }
}
```

**Step 2: Create mod.rs**

Create `ssmd-rust/crates/middleware/src/memory/mod.rs`:

```rust
//! In-memory implementations for testing

pub mod transport;

pub use transport::InMemoryTransport;
```

**Step 3: Update lib.rs**

Add to `ssmd-rust/crates/middleware/src/lib.rs`:

```rust
pub mod memory;

pub use memory::InMemoryTransport;
```

**Step 4: Run tests**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: PASS (all tests including new InMemoryTransport tests)

**Step 5: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add InMemoryTransport implementation"
```

---

## Task 7: InMemoryStorage Implementation

**Files:**
- Create: `ssmd-rust/crates/middleware/src/memory/storage.rs`
- Modify: `ssmd-rust/crates/middleware/src/memory/mod.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Write InMemoryStorage**

Create `ssmd-rust/crates/middleware/src/memory/storage.rs`:

```rust
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::StorageError;
use crate::storage::{ObjectMeta, Storage};

/// In-memory storage for testing
pub struct InMemoryStorage {
    // buckets -> keys -> (data, meta)
    data: Arc<RwLock<HashMap<String, HashMap<String, (Bytes, ObjectMeta)>>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
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
        let bucket_data = store.entry(bucket.to_string()).or_insert_with(HashMap::new);
        bucket_data.insert(key.to_string(), (data, meta.clone()));

        Ok(meta)
    }

    async fn get(&self, bucket: &str, key: &str) -> Result<Bytes, StorageError> {
        let store = self.data.read().await;
        store
            .get(bucket)
            .and_then(|b| b.get(key))
            .map(|(data, _)| data.clone())
            .ok_or_else(|| StorageError::NotFound(format!("{}/{}", bucket, key)))
    }

    async fn exists(&self, bucket: &str, key: &str) -> Result<bool, StorageError> {
        let store = self.data.read().await;
        Ok(store
            .get(bucket)
            .map(|b| b.contains_key(key))
            .unwrap_or(false))
    }

    async fn head(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        let store = self.data.read().await;
        store
            .get(bucket)
            .and_then(|b| b.get(key))
            .map(|(_, meta)| meta.clone())
            .ok_or_else(|| StorageError::NotFound(format!("{}/{}", bucket, key)))
    }

    async fn delete(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        let mut store = self.data.write().await;
        if let Some(bucket_data) = store.get_mut(bucket) {
            bucket_data.remove(key);
        }
        Ok(())
    }

    async fn list(&self, bucket: &str, prefix: &str) -> Result<Vec<ObjectMeta>, StorageError> {
        let store = self.data.read().await;
        let metas = store
            .get(bucket)
            .map(|b| {
                b.iter()
                    .filter(|(k, _)| k.starts_with(prefix))
                    .map(|(_, (_, meta))| meta.clone())
                    .collect()
            })
            .unwrap_or_default();
        Ok(metas)
    }

    async fn create_bucket(&self, bucket: &str) -> Result<(), StorageError> {
        let mut store = self.data.write().await;
        store.entry(bucket.to_string()).or_insert_with(HashMap::new);
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
        let meta = storage.put("test-bucket", "file.txt", data.clone()).await.unwrap();

        assert_eq!(meta.key, "file.txt");
        assert_eq!(meta.size, 11);

        let retrieved = storage.get("test-bucket", "file.txt").await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_exists() {
        let storage = InMemoryStorage::new();

        assert!(!storage.exists("bucket", "key").await.unwrap());

        storage.put("bucket", "key", Bytes::from("x")).await.unwrap();
        assert!(storage.exists("bucket", "key").await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let storage = InMemoryStorage::new();

        storage.put("bucket", "key", Bytes::from("x")).await.unwrap();
        assert!(storage.exists("bucket", "key").await.unwrap());

        storage.delete("bucket", "key").await.unwrap();
        assert!(!storage.exists("bucket", "key").await.unwrap());
    }

    #[tokio::test]
    async fn test_list_with_prefix() {
        let storage = InMemoryStorage::new();

        storage.put("bucket", "2025/12/a.txt", Bytes::from("a")).await.unwrap();
        storage.put("bucket", "2025/12/b.txt", Bytes::from("b")).await.unwrap();
        storage.put("bucket", "2025/11/c.txt", Bytes::from("c")).await.unwrap();

        let dec_files = storage.list("bucket", "2025/12/").await.unwrap();
        assert_eq!(dec_files.len(), 2);

        let all_files = storage.list("bucket", "2025/").await.unwrap();
        assert_eq!(all_files.len(), 3);
    }

    #[tokio::test]
    async fn test_not_found() {
        let storage = InMemoryStorage::new();

        let result = storage.get("bucket", "missing").await;
        assert!(matches!(result, Err(StorageError::NotFound(_))));
    }
}
```

**Step 2: Add md5 dependency**

Update `ssmd-rust/crates/middleware/Cargo.toml`:

```toml
[dependencies]
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
bytes = { workspace = true }
tracing = { workspace = true }
futures-util = { workspace = true }
md5 = "0.7"
```

**Step 3: Update mod.rs**

Update `ssmd-rust/crates/middleware/src/memory/mod.rs`:

```rust
//! In-memory implementations for testing

pub mod storage;
pub mod transport;

pub use storage::InMemoryStorage;
pub use transport::InMemoryTransport;
```

**Step 4: Update lib.rs exports**

Update the lib.rs to export InMemoryStorage:

```rust
pub use memory::{InMemoryStorage, InMemoryTransport};
```

**Step 5: Run tests**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: PASS

**Step 6: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add InMemoryStorage implementation"
```

---

## Task 8: InMemoryCache Implementation

**Files:**
- Create: `ssmd-rust/crates/middleware/src/memory/cache.rs`
- Modify: `ssmd-rust/crates/middleware/src/memory/mod.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Write InMemoryCache**

Create `ssmd-rust/crates/middleware/src/memory/cache.rs`:

```rust
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

/// In-memory cache for testing
pub struct InMemoryCache {
    data: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl InMemoryCache {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Cache for InMemoryCache {
    async fn get(&self, key: &str) -> Result<Option<Bytes>, CacheError> {
        let data = self.data.read().await;
        Ok(data.get(key).and_then(|e| {
            if e.is_expired() {
                None
            } else {
                Some(e.value.clone())
            }
        }))
    }

    async fn set(&self, key: &str, value: Bytes, ttl: Option<Duration>) -> Result<(), CacheError> {
        let expires_at = ttl.map(|d| Instant::now() + d);
        let entry = CacheEntry { value, expires_at };

        let mut data = self.data.write().await;
        data.insert(key.to_string(), entry);
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

    async fn set_nx(
        &self,
        key: &str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> Result<bool, CacheError> {
        let mut data = self.data.write().await;

        // Check if key exists and is not expired
        if let Some(entry) = data.get(key) {
            if !entry.is_expired() {
                return Ok(false);
            }
        }

        let expires_at = ttl.map(|d| Instant::now() + d);
        let entry = CacheEntry { value, expires_at };
        data.insert(key.to_string(), entry);
        Ok(true)
    }

    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<Bytes>>, CacheError> {
        let data = self.data.read().await;
        Ok(keys
            .iter()
            .map(|k| {
                data.get(*k).and_then(|e| {
                    if e.is_expired() {
                        None
                    } else {
                        Some(e.value.clone())
                    }
                })
            })
            .collect())
    }

    async fn mset(&self, pairs: &[(&str, Bytes)]) -> Result<(), CacheError> {
        let mut data = self.data.write().await;
        for (key, value) in pairs {
            let entry = CacheEntry {
                value: value.clone(),
                expires_at: None,
            };
            data.insert(key.to_string(), entry);
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

        assert!(cache.get("key").await.unwrap().is_none());

        cache.set("key", Bytes::from("value"), None).await.unwrap();
        assert_eq!(cache.get("key").await.unwrap(), Some(Bytes::from("value")));
    }

    #[tokio::test]
    async fn test_delete() {
        let cache = InMemoryCache::new();

        cache.set("key", Bytes::from("value"), None).await.unwrap();
        cache.delete("key").await.unwrap();
        assert!(cache.get("key").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_exists() {
        let cache = InMemoryCache::new();

        assert!(!cache.exists("key").await.unwrap());
        cache.set("key", Bytes::from("value"), None).await.unwrap();
        assert!(cache.exists("key").await.unwrap());
    }

    #[tokio::test]
    async fn test_set_nx() {
        let cache = InMemoryCache::new();

        // First set_nx should succeed
        assert!(cache.set_nx("key", Bytes::from("v1"), None).await.unwrap());

        // Second set_nx should fail (key exists)
        assert!(!cache.set_nx("key", Bytes::from("v2"), None).await.unwrap());

        // Value should still be v1
        assert_eq!(cache.get("key").await.unwrap(), Some(Bytes::from("v1")));
    }

    #[tokio::test]
    async fn test_mget_mset() {
        let cache = InMemoryCache::new();

        cache
            .mset(&[("a", Bytes::from("1")), ("b", Bytes::from("2"))])
            .await
            .unwrap();

        let values = cache.mget(&["a", "b", "c"]).await.unwrap();
        assert_eq!(values[0], Some(Bytes::from("1")));
        assert_eq!(values[1], Some(Bytes::from("2")));
        assert_eq!(values[2], None);
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        let cache = InMemoryCache::new();

        // Set with very short TTL
        cache
            .set("key", Bytes::from("value"), Some(Duration::from_millis(1)))
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Should be gone
        assert!(cache.get("key").await.unwrap().is_none());
        assert!(!cache.exists("key").await.unwrap());
    }
}
```

**Step 2: Update mod.rs**

Update `ssmd-rust/crates/middleware/src/memory/mod.rs`:

```rust
//! In-memory implementations for testing

pub mod cache;
pub mod storage;
pub mod transport;

pub use cache::InMemoryCache;
pub use storage::InMemoryStorage;
pub use transport::InMemoryTransport;
```

**Step 3: Update lib.rs exports**

Update lib.rs to export InMemoryCache:

```rust
pub use memory::{InMemoryCache, InMemoryStorage, InMemoryTransport};
```

**Step 4: Run tests**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: PASS

**Step 5: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add InMemoryCache implementation"
```

---

## Task 9: InMemoryJournal Implementation

**Files:**
- Create: `ssmd-rust/crates/middleware/src/memory/journal.rs`
- Modify: `ssmd-rust/crates/middleware/src/memory/mod.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Write InMemoryJournal**

Create `ssmd-rust/crates/middleware/src/memory/journal.rs`:

```rust
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::error::JournalError;
use crate::journal::{Journal, JournalEntry, JournalPosition, JournalReader, TopicConfig};

/// In-memory journal for testing
pub struct InMemoryJournal {
    topics: Arc<RwLock<HashMap<String, Vec<JournalEntry>>>>,
    sequence: Arc<Mutex<u64>>,
}

impl InMemoryJournal {
    pub fn new() -> Self {
        Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
            sequence: Arc::new(Mutex::new(0)),
        }
    }

    async fn next_sequence(&self) -> u64 {
        let mut seq = self.sequence.lock().await;
        *seq += 1;
        *seq
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
}

impl Default for InMemoryJournal {
    fn default() -> Self {
        Self::new()
    }
}

struct InMemoryJournalReader {
    entries: Vec<JournalEntry>,
    position: usize,
}

#[async_trait]
impl JournalReader for InMemoryJournalReader {
    async fn next(&mut self) -> Result<Option<JournalEntry>, JournalError> {
        if self.position >= self.entries.len() {
            Ok(None)
        } else {
            let entry = self.entries[self.position].clone();
            self.position += 1;
            Ok(Some(entry))
        }
    }

    async fn seek(&mut self, position: JournalPosition) -> Result<(), JournalError> {
        match position {
            JournalPosition::Beginning => {
                self.position = 0;
            }
            JournalPosition::End => {
                self.position = self.entries.len();
            }
            JournalPosition::Sequence(seq) => {
                self.position = self
                    .entries
                    .iter()
                    .position(|e| e.sequence >= seq)
                    .unwrap_or(self.entries.len());
            }
            JournalPosition::Time(ts) => {
                self.position = self
                    .entries
                    .iter()
                    .position(|e| e.timestamp >= ts)
                    .unwrap_or(self.entries.len());
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Journal for InMemoryJournal {
    async fn append(
        &self,
        topic: &str,
        key: Option<Bytes>,
        payload: Bytes,
    ) -> Result<u64, JournalError> {
        self.append_with_headers(topic, key, payload, HashMap::new())
            .await
    }

    async fn append_with_headers(
        &self,
        topic: &str,
        key: Option<Bytes>,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<u64, JournalError> {
        let seq = self.next_sequence().await;
        let entry = JournalEntry {
            sequence: seq,
            timestamp: Self::now_millis(),
            topic: topic.to_string(),
            key,
            payload,
            headers,
        };

        let mut topics = self.topics.write().await;
        let entries = topics.entry(topic.to_string()).or_insert_with(Vec::new);
        entries.push(entry);

        Ok(seq)
    }

    async fn reader(
        &self,
        topic: &str,
        position: JournalPosition,
    ) -> Result<Box<dyn JournalReader>, JournalError> {
        let topics = self.topics.read().await;
        let entries = topics
            .get(topic)
            .cloned()
            .unwrap_or_default();

        let mut reader = InMemoryJournalReader {
            entries,
            position: 0,
        };
        reader.seek(position).await?;

        Ok(Box::new(reader))
    }

    async fn end_position(&self, topic: &str) -> Result<u64, JournalError> {
        let topics = self.topics.read().await;
        Ok(topics
            .get(topic)
            .and_then(|entries| entries.last().map(|e| e.sequence))
            .unwrap_or(0))
    }

    async fn create_topic(&self, config: TopicConfig) -> Result<(), JournalError> {
        let mut topics = self.topics.write().await;
        topics.entry(config.name).or_insert_with(Vec::new);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_append_and_read() {
        let journal = InMemoryJournal::new();

        let seq1 = journal
            .append("test.topic", None, Bytes::from("message 1"))
            .await
            .unwrap();
        let seq2 = journal
            .append("test.topic", None, Bytes::from("message 2"))
            .await
            .unwrap();

        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);

        let mut reader = journal
            .reader("test.topic", JournalPosition::Beginning)
            .await
            .unwrap();

        let entry1 = reader.next().await.unwrap().unwrap();
        assert_eq!(entry1.sequence, 1);
        assert_eq!(entry1.payload, Bytes::from("message 1"));

        let entry2 = reader.next().await.unwrap().unwrap();
        assert_eq!(entry2.sequence, 2);
        assert_eq!(entry2.payload, Bytes::from("message 2"));

        // No more entries
        assert!(reader.next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_seek_to_sequence() {
        let journal = InMemoryJournal::new();

        journal.append("topic", None, Bytes::from("1")).await.unwrap();
        journal.append("topic", None, Bytes::from("2")).await.unwrap();
        journal.append("topic", None, Bytes::from("3")).await.unwrap();

        let mut reader = journal
            .reader("topic", JournalPosition::Sequence(2))
            .await
            .unwrap();

        let entry = reader.next().await.unwrap().unwrap();
        assert_eq!(entry.sequence, 2);
    }

    #[tokio::test]
    async fn test_end_position() {
        let journal = InMemoryJournal::new();

        assert_eq!(journal.end_position("topic").await.unwrap(), 0);

        journal.append("topic", None, Bytes::from("x")).await.unwrap();
        journal.append("topic", None, Bytes::from("y")).await.unwrap();

        assert_eq!(journal.end_position("topic").await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_append_with_key_and_headers() {
        let journal = InMemoryJournal::new();

        let mut headers = HashMap::new();
        headers.insert("type".to_string(), "trade".to_string());

        journal
            .append_with_headers(
                "topic",
                Some(Bytes::from("BTCUSD")),
                Bytes::from(r#"{"price":100}"#),
                headers,
            )
            .await
            .unwrap();

        let mut reader = journal
            .reader("topic", JournalPosition::Beginning)
            .await
            .unwrap();

        let entry = reader.next().await.unwrap().unwrap();
        assert_eq!(entry.key, Some(Bytes::from("BTCUSD")));
        assert_eq!(entry.headers.get("type"), Some(&"trade".to_string()));
    }
}
```

**Step 2: Update mod.rs**

Update `ssmd-rust/crates/middleware/src/memory/mod.rs`:

```rust
//! In-memory implementations for testing

pub mod cache;
pub mod journal;
pub mod storage;
pub mod transport;

pub use cache::InMemoryCache;
pub use journal::InMemoryJournal;
pub use storage::InMemoryStorage;
pub use transport::InMemoryTransport;
```

**Step 3: Update lib.rs exports**

Update lib.rs to export InMemoryJournal:

```rust
pub use memory::{InMemoryCache, InMemoryJournal, InMemoryStorage, InMemoryTransport};
```

**Step 4: Run tests**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: PASS

**Step 5: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add InMemoryJournal implementation"
```

---

## Task 10: MiddlewareFactory for Runtime Selection

**Files:**
- Create: `ssmd-rust/crates/middleware/src/factory.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`
- Modify: `ssmd-rust/crates/middleware/Cargo.toml`

**Step 1: Write the factory**

Create `ssmd-rust/crates/middleware/src/factory.rs`:

```rust
use std::sync::Arc;

use ssmd_metadata::{CacheType, Environment, StorageType, TransportType};

use crate::cache::Cache;
use crate::journal::Journal;
use crate::memory::{InMemoryCache, InMemoryJournal, InMemoryStorage, InMemoryTransport};
use crate::storage::Storage;
use crate::transport::Transport;

/// Error creating middleware
#[derive(Debug, thiserror::Error)]
pub enum FactoryError {
    #[error("unsupported transport type: {0:?}")]
    UnsupportedTransport(TransportType),
    #[error("unsupported storage type: {0:?}")]
    UnsupportedStorage(StorageType),
    #[error("unsupported cache type: {0:?}")]
    UnsupportedCache(CacheType),
    #[error("configuration error: {0}")]
    ConfigError(String),
}

/// Factory for creating middleware instances based on environment config
pub struct MiddlewareFactory;

impl MiddlewareFactory {
    /// Create a transport based on environment configuration
    pub fn create_transport(env: &Environment) -> Result<Arc<dyn Transport>, FactoryError> {
        match env.transport.transport_type {
            TransportType::Memory => Ok(Arc::new(InMemoryTransport::new())),
            TransportType::Nats => {
                // NATS implementation will come later
                Err(FactoryError::UnsupportedTransport(TransportType::Nats))
            }
            TransportType::Mqtt => {
                Err(FactoryError::UnsupportedTransport(TransportType::Mqtt))
            }
        }
    }

    /// Create a storage based on environment configuration
    pub fn create_storage(env: &Environment) -> Result<Arc<dyn Storage>, FactoryError> {
        match env.storage.storage_type {
            StorageType::Local => {
                // Local storage maps to in-memory for now
                // Real LocalStorage (file-based) will come later
                Ok(Arc::new(InMemoryStorage::new()))
            }
            StorageType::S3 => {
                Err(FactoryError::UnsupportedStorage(StorageType::S3))
            }
        }
    }

    /// Create a cache based on environment configuration
    pub fn create_cache(env: &Environment) -> Result<Arc<dyn Cache>, FactoryError> {
        let cache_type = env
            .cache
            .as_ref()
            .map(|c| c.cache_type.clone())
            .unwrap_or(CacheType::Memory);

        match cache_type {
            CacheType::Memory => Ok(Arc::new(InMemoryCache::new())),
            CacheType::Redis => {
                Err(FactoryError::UnsupportedCache(CacheType::Redis))
            }
        }
    }

    /// Create a journal (always in-memory for now)
    pub fn create_journal() -> Arc<dyn Journal> {
        Arc::new(InMemoryJournal::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_metadata::{StorageConfig, TransportConfig};

    fn make_test_env() -> Environment {
        Environment {
            name: "test".to_string(),
            feed: "kalshi".to_string(),
            schema: "trade:v1".to_string(),
            schedule: None,
            keys: None,
            transport: TransportConfig {
                transport_type: TransportType::Memory,
                url: None,
            },
            storage: StorageConfig {
                storage_type: StorageType::Local,
                path: Some("/tmp/test".to_string()),
                bucket: None,
                region: None,
            },
            cache: None,
        }
    }

    #[test]
    fn test_create_memory_transport() {
        let env = make_test_env();
        let transport = MiddlewareFactory::create_transport(&env).unwrap();
        // Just verify we got a transport
        drop(transport);
    }

    #[test]
    fn test_create_local_storage() {
        let env = make_test_env();
        let storage = MiddlewareFactory::create_storage(&env).unwrap();
        drop(storage);
    }

    #[test]
    fn test_create_memory_cache() {
        let env = make_test_env();
        let cache = MiddlewareFactory::create_cache(&env).unwrap();
        drop(cache);
    }

    #[test]
    fn test_create_journal() {
        let journal = MiddlewareFactory::create_journal();
        drop(journal);
    }

    #[test]
    fn test_unsupported_transport() {
        let mut env = make_test_env();
        env.transport.transport_type = TransportType::Nats;

        let result = MiddlewareFactory::create_transport(&env);
        assert!(matches!(result, Err(FactoryError::UnsupportedTransport(_))));
    }
}
```

**Step 2: Add ssmd-metadata dependency**

Update `ssmd-rust/crates/middleware/Cargo.toml`:

```toml
[dependencies]
ssmd-metadata = { path = "../metadata" }
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
bytes = { workspace = true }
tracing = { workspace = true }
futures-util = { workspace = true }
md5 = "0.7"
```

**Step 3: Update lib.rs**

Add to `ssmd-rust/crates/middleware/src/lib.rs`:

```rust
pub mod factory;

pub use factory::{FactoryError, MiddlewareFactory};
```

**Step 4: Run tests**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware`
Expected: PASS

**Step 5: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): add MiddlewareFactory for runtime selection"
```

---

## Task 11: Create ssmd-schema Crate with Cap'n Proto

**Files:**
- Create: `ssmd-rust/crates/schema/Cargo.toml`
- Create: `ssmd-rust/crates/schema/build.rs`
- Create: `ssmd-rust/crates/schema/src/lib.rs`
- Create: `ssmd-rust/crates/schema/schemas/trade.capnp`
- Modify: `ssmd-rust/Cargo.toml`

**Step 1: Add capnp to workspace dependencies**

Update `ssmd-rust/Cargo.toml`:

```toml
members = [
    "crates/metadata",
    "crates/middleware",
    "crates/schema",
    "crates/connector",
    "crates/ssmd-connector",
]

[workspace.dependencies]
# ... existing deps ...
capnp = "0.19"
capnpc = "0.19"
```

**Step 2: Create Cargo.toml**

Create `ssmd-rust/crates/schema/Cargo.toml`:

```toml
[package]
name = "ssmd-schema"
version.workspace = true
edition.workspace = true

[dependencies]
capnp = { workspace = true }

[build-dependencies]
capnpc = { workspace = true }
```

**Step 3: Create the Cap'n Proto schema file**

Create `ssmd-rust/crates/schema/schemas/trade.capnp`:

```capnp
@0xa1b2c3d4e5f60001;

enum Side {
    buy @0;
    sell @1;
}

struct Trade {
    timestamp @0 :UInt64;        # Unix nanos
    ticker @1 :Text;
    price @2 :Float64;
    size @3 :UInt32;
    side @4 :Side;
    tradeId @5 :Text;
}

struct Level {
    price @0 :Float64;
    size @1 :UInt32;
}

struct OrderBookUpdate {
    timestamp @0 :UInt64;        # Unix nanos
    ticker @1 :Text;
    bids @2 :List(Level);
    asks @3 :List(Level);
}

enum MarketStatus {
    open @0;
    closed @1;
    halted @2;
}

struct MarketStatusUpdate {
    timestamp @0 :UInt64;
    ticker @1 :Text;
    status @2 :MarketStatus;
}
```

**Step 4: Create build.rs**

Create `ssmd-rust/crates/schema/build.rs`:

```rust
fn main() {
    capnpc::CompilerCommand::new()
        .file("schemas/trade.capnp")
        .run()
        .expect("compiling schema");
}
```

**Step 5: Create lib.rs**

Create `ssmd-rust/crates/schema/src/lib.rs`:

```rust
//! ssmd-schema: Cap'n Proto generated types for market data
//!
//! This crate contains the generated Rust types from Cap'n Proto schemas.

#[allow(dead_code)]
mod trade_capnp {
    include!(concat!(env!("OUT_DIR"), "/trade_capnp.rs"));
}

pub use trade_capnp::*;

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::message::Builder;

    #[test]
    fn test_build_trade() {
        let mut message = Builder::new_default();
        {
            let mut trade = message.init_root::<trade::Builder>();
            trade.set_timestamp(1703318400000000000); // nanos
            trade.set_ticker("BTCUSD");
            trade.set_price(100.50);
            trade.set_size(10);
            trade.set_side(side::Buy);
            trade.set_trade_id("trade-001");
        }

        let reader = message.get_root_as_reader::<trade::Reader>().unwrap();
        assert_eq!(reader.get_timestamp(), 1703318400000000000);
        assert_eq!(reader.get_ticker().unwrap(), "BTCUSD");
        assert_eq!(reader.get_price(), 100.50);
        assert_eq!(reader.get_size(), 10);
        assert!(matches!(reader.get_side().unwrap(), side::Buy));
    }

    #[test]
    fn test_build_order_book_update() {
        let mut message = Builder::new_default();
        {
            let mut update = message.init_root::<order_book_update::Builder>();
            update.set_timestamp(1703318400000000000);
            update.set_ticker("BTCUSD");

            let mut bids = update.init_bids(2);
            bids.reborrow().get(0).set_price(100.0);
            bids.reborrow().get(0).set_size(50);
            bids.reborrow().get(1).set_price(99.0);
            bids.reborrow().get(1).set_size(100);

            let mut asks = update.init_asks(1);
            asks.get(0).set_price(101.0);
            asks.get(0).set_size(25);
        }

        let reader = message
            .get_root_as_reader::<order_book_update::Reader>()
            .unwrap();
        assert_eq!(reader.get_ticker().unwrap(), "BTCUSD");

        let bids = reader.get_bids().unwrap();
        assert_eq!(bids.len(), 2);
        assert_eq!(bids.get(0).get_price(), 100.0);
    }
}
```

**Step 6: Build to verify Cap'n Proto compilation**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo build -p ssmd-schema`
Expected: Build succeeds (requires capnp compiler installed)

Note: If `capnp` compiler is not installed, run:
```bash
sudo apt-get update && sudo apt-get install -y capnproto
```

**Step 7: Run tests**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-schema`
Expected: PASS

**Step 8: Commit**

```bash
git add ssmd-rust/Cargo.toml ssmd-rust/crates/schema/
git commit -m "feat(schema): add ssmd-schema crate with Cap'n Proto trade types"
```

---

## Task 12: Update Connector to Use Middleware

**Files:**
- Modify: `ssmd-rust/crates/connector/Cargo.toml`
- Modify: `ssmd-rust/crates/connector/src/lib.rs`
- Create: `ssmd-rust/crates/connector/src/publisher.rs`

**Step 1: Add dependencies**

Update `ssmd-rust/crates/connector/Cargo.toml`:

```toml
[dependencies]
ssmd-metadata = { path = "../metadata" }
ssmd-middleware = { path = "../middleware" }
ssmd-schema = { path = "../schema" }
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
serde_yaml = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
axum = { workspace = true }
tokio-tungstenite = { workspace = true }
futures-util = { workspace = true }
url = { workspace = true }
bytes = { workspace = true }
capnp = { workspace = true }
```

**Step 2: Create publisher module**

Create `ssmd-rust/crates/connector/src/publisher.rs`:

```rust
//! Publisher for sending normalized data to transport

use std::sync::Arc;

use bytes::Bytes;
use capnp::message::Builder;
use ssmd_middleware::{Transport, TransportError};
use ssmd_schema::{side, trade};

/// Trade data for publishing
#[derive(Debug, Clone)]
pub struct TradeData {
    pub timestamp_nanos: u64,
    pub ticker: String,
    pub price: f64,
    pub size: u32,
    pub side: TradeSide,
    pub trade_id: String,
}

#[derive(Debug, Clone, Copy)]
pub enum TradeSide {
    Buy,
    Sell,
}

/// Publisher for sending Cap'n Proto encoded messages to transport
pub struct Publisher {
    transport: Arc<dyn Transport>,
    env_prefix: String,
    feed_name: String,
}

impl Publisher {
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<String>,
        feed_name: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            env_prefix: env_name.into(),
            feed_name: feed_name.into(),
        }
    }

    /// Publish a trade to the transport
    pub async fn publish_trade(&self, trade_data: &TradeData) -> Result<(), TransportError> {
        // Build Cap'n Proto message
        let mut message = Builder::new_default();
        {
            let mut trade_builder = message.init_root::<trade::Builder>();
            trade_builder.set_timestamp(trade_data.timestamp_nanos);
            trade_builder.set_ticker(&trade_data.ticker);
            trade_builder.set_price(trade_data.price);
            trade_builder.set_size(trade_data.size);
            trade_builder.set_side(match trade_data.side {
                TradeSide::Buy => side::Buy,
                TradeSide::Sell => side::Sell,
            });
            trade_builder.set_trade_id(&trade_data.trade_id);
        }

        // Serialize to bytes
        let mut output = Vec::new();
        capnp::serialize::write_message(&mut output, &message)
            .map_err(|e| TransportError::PublishFailed(e.to_string()))?;

        // Publish to transport
        let subject = format!(
            "{}.{}.trade.{}",
            self.env_prefix, self.feed_name, trade_data.ticker
        );
        self.transport.publish(&subject, Bytes::from(output)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_middleware::InMemoryTransport;

    #[tokio::test]
    async fn test_publish_trade() {
        let transport = Arc::new(InMemoryTransport::new());
        let publisher = Publisher::new(transport.clone(), "kalshi-dev", "kalshi");

        // Subscribe before publishing
        let mut sub = transport.subscribe("kalshi-dev.kalshi.trade.BTCUSD").await.unwrap();

        let trade = TradeData {
            timestamp_nanos: 1703318400000000000,
            ticker: "BTCUSD".to_string(),
            price: 100.50,
            size: 10,
            side: TradeSide::Buy,
            trade_id: "trade-001".to_string(),
        };

        publisher.publish_trade(&trade).await.unwrap();

        // Receive and verify
        let msg = sub.next().await.unwrap();
        assert_eq!(msg.subject, "kalshi-dev.kalshi.trade.BTCUSD");
        assert!(!msg.payload.is_empty());

        // Deserialize and verify
        let reader = capnp::serialize::read_message_from_flat_slice(
            &mut msg.payload.as_ref(),
            capnp::message::ReaderOptions::new(),
        )
        .unwrap();
        let trade_reader = reader.get_root::<trade::Reader>().unwrap();
        assert_eq!(trade_reader.get_ticker().unwrap(), "BTCUSD");
        assert_eq!(trade_reader.get_price(), 100.50);
    }
}
```

**Step 3: Update lib.rs exports**

Add to `ssmd-rust/crates/connector/src/lib.rs`:

```rust
pub mod publisher;

pub use publisher::{Publisher, TradeData, TradeSide};
```

**Step 4: Run tests**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-connector-lib`
Expected: PASS

**Step 5: Commit**

```bash
git add ssmd-rust/crates/connector/
git commit -m "feat(connector): add Publisher for Cap'n Proto trade messages"
```

---

## Task 13: Update Makefile and Run Full Test

**Files:**
- Modify: `Makefile`

**Step 1: Add capnproto install check to Makefile**

The Makefile already has rust targets. Verify everything builds and tests pass.

**Step 2: Run full build and test**

Run: `cd /workspaces/ssmd && make all-test`
Expected: All Go and Rust tests pass

**Step 3: Run clippy**

Run: `cd /workspaces/ssmd && make rust-clippy`
Expected: No errors

**Step 4: Commit any remaining changes**

```bash
git add -A
git commit -m "chore: final cleanup for middleware and schema"
```

---

## Task 14: Update exchanges schema file

**Files:**
- Modify: `exchanges/schemas/trade.capnp`

**Step 1: Copy the schema content**

Update the empty `exchanges/schemas/trade.capnp` with the actual schema:

```capnp
@0xa1b2c3d4e5f60001;

enum Side {
    buy @0;
    sell @1;
}

struct Trade {
    timestamp @0 :UInt64;        # Unix nanos
    ticker @1 :Text;
    price @2 :Float64;
    size @3 :UInt32;
    side @4 :Side;
    tradeId @5 :Text;
}

struct Level {
    price @0 :Float64;
    size @1 :UInt32;
}

struct OrderBookUpdate {
    timestamp @0 :UInt64;        # Unix nanos
    ticker @1 :Text;
    bids @2 :List(Level);
    asks @3 :List(Level);
}

enum MarketStatus {
    open @0;
    closed @1;
    halted @2;
}

struct MarketStatusUpdate {
    timestamp @0 :UInt64;
    ticker @1 :Text;
    status @2 :MarketStatus;
}
```

**Step 2: Update the hash in trade.yaml**

After updating the schema, compute the new hash and update `exchanges/schemas/trade.yaml`:

Run: `shasum -a 256 exchanges/schemas/trade.capnp`

Update the hash in trade.yaml with the result.

**Step 3: Commit**

```bash
git add exchanges/schemas/
git commit -m "feat(schema): add Cap'n Proto trade schema definition"
```

---

## Task 15: Final lib.rs Cleanup

**Files:**
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Ensure clean exports**

Verify `ssmd-rust/crates/middleware/src/lib.rs` has all exports:

```rust
//! ssmd-middleware: Pluggable middleware abstractions
//!
//! Provides trait-based abstractions for Transport, Storage, Cache, and Journal
//! with in-memory implementations for testing.

pub mod cache;
pub mod error;
pub mod factory;
pub mod journal;
pub mod memory;
pub mod storage;
pub mod transport;

// Error types
pub use error::{CacheError, JournalError, StorageError, TransportError};

// Trait definitions
pub use cache::Cache;
pub use journal::{Journal, JournalEntry, JournalPosition, JournalReader, TopicConfig};
pub use storage::{ObjectMeta, Storage};
pub use transport::{Subscription, Transport, TransportMessage};

// In-memory implementations
pub use memory::{InMemoryCache, InMemoryJournal, InMemoryStorage, InMemoryTransport};

// Factory
pub use factory::{FactoryError, MiddlewareFactory};
```

**Step 2: Run final tests**

Run: `cd /workspaces/ssmd && make all-test`
Expected: PASS

**Step 3: Commit if needed**

```bash
git add -A
git commit -m "chore: finalize middleware lib exports"
```

---

## Summary

This plan implements:

1. **ssmd-middleware crate** with:
   - Transport trait + InMemoryTransport
   - Storage trait + InMemoryStorage
   - Cache trait + InMemoryCache
   - Journal trait + InMemoryJournal
   - MiddlewareFactory for runtime selection

2. **ssmd-schema crate** with:
   - Cap'n Proto schema for Trade, OrderBookUpdate, MarketStatusUpdate
   - Generated Rust types via build.rs

3. **Connector updates**:
   - Publisher for sending Cap'n Proto encoded messages to transport
   - Integration with middleware crate

After this plan, the system will be ready for actual WebSocket connection implementation in the next phase.
