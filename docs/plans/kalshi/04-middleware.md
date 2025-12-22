# ssmd: Kalshi Design - Middleware Abstractions

All infrastructure dependencies are behind traits. Implementations are selected at deployment time via environment configuration - no code changes required to swap backends.

## Design Principles

1. **Trait-based** - Rust traits define the contract, implementations are pluggable
2. **Config-driven** - Environment YAML selects which implementation to use
3. **Runtime resolution** - Factory functions create the right implementation at startup
4. **Test-friendly** - In-memory implementations for testing without infrastructure

## Abstraction Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                        APPLICATION LAYER                            │
│   Connector    Archiver    Gateway    Worker                        │
└───────┬────────────┬──────────┬─────────┬───────────────────────────┘
        │            │          │         │
┌───────▼────────────▼──────────▼─────────▼───────────────────────────┐
│                      ABSTRACTION LAYER                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐             │
│  │Transport │  │ Storage  │  │  Cache   │  │ Journal  │             │
│  │  trait   │  │  trait   │  │  trait   │  │  trait   │             │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘             │
└───────┼─────────────┼───────────────┼───────────┼───────────────────┘
        │             │               │           │
┌───────▼─────────────▼───────────────▼───────────▼───────────────────┐
│                     IMPLEMENTATION LAYER                             │
│  NATS/Aeron/     S3/Local/       Redis/        NATS/                │
│  Chronicle       Garage          Memory        Chronicle            │
└─────────────────────────────────────────────────────────────────────┘
```

## 1. Transport Trait

Pub/sub messaging for streaming data between components.

```rust
use async_trait::async_trait;
use bytes::Bytes;

/// Message envelope with metadata
pub struct Message {
    pub subject: String,
    pub payload: Bytes,
    pub headers: HashMap<String, String>,
    pub timestamp: u64,
    pub sequence: Option<u64>,
}

/// Acknowledgment for reliable delivery
pub struct Ack {
    pub sequence: u64,
}

/// Subscription handle
#[async_trait]
pub trait Subscription: Send + Sync {
    async fn next(&mut self) -> Result<Message, TransportError>;
    async fn ack(&self, ack: Ack) -> Result<(), TransportError>;
    async fn unsubscribe(self) -> Result<(), TransportError>;
}

/// Transport abstraction
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

    /// Subscribe with consumer group (load balanced)
    async fn queue_subscribe(
        &self,
        subject: &str,
        queue: &str,
    ) -> Result<Box<dyn Subscription>, TransportError>;

    /// Request/reply pattern
    async fn request(
        &self,
        subject: &str,
        payload: Bytes,
        timeout: Duration,
    ) -> Result<Message, TransportError>;

    /// Create a durable stream (for replay)
    async fn create_stream(&self, config: StreamConfig) -> Result<(), TransportError>;

    /// Subscribe from a stream position (for replay)
    async fn subscribe_from(
        &self,
        stream: &str,
        position: StreamPosition,
    ) -> Result<Box<dyn Subscription>, TransportError>;
}

pub struct StreamConfig {
    pub name: String,
    pub subjects: Vec<String>,
    pub retention: RetentionPolicy,
    pub max_bytes: Option<u64>,
    pub max_age: Option<Duration>,
}

pub enum StreamPosition {
    Beginning,
    End,
    Sequence(u64),
    Time(u64),
}

pub enum RetentionPolicy {
    Limits,      // Delete when limits exceeded
    Interest,    // Delete when no consumers
    WorkQueue,   // Delete after ack
}
```

**Implementations:**

| Implementation | Crate | Use Case |
|----------------|-------|----------|
| `NatsTransport` | `async-nats` | Default, JetStream for durability |
| `MqttTransport` | `rumqttc` | Message-oriented middleware, IoT-friendly |
| `AeronTransport` | `aeron-rs` | Low-latency, reliable multicast |
| `ChronicleTransport` | FFI | On-prem, shared memory |
| `InMemoryTransport` | built-in | Testing |

## 2. Storage Trait

Object storage for raw and normalized data files.

```rust
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;

/// Object metadata
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub last_modified: u64,
    pub etag: Option<String>,
    pub content_type: Option<String>,
}

/// Streaming upload handle
#[async_trait]
pub trait UploadStream: Send + Sync {
    async fn write(&mut self, chunk: Bytes) -> Result<(), StorageError>;
    async fn finish(self) -> Result<ObjectMeta, StorageError>;
    async fn abort(self) -> Result<(), StorageError>;
}

/// Storage abstraction
#[async_trait]
pub trait Storage: Send + Sync {
    /// Put an object (small files)
    async fn put(&self, bucket: &str, key: &str, data: Bytes) -> Result<ObjectMeta, StorageError>;

    /// Put with streaming (large files)
    async fn put_stream(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Box<dyn UploadStream>, StorageError>;

    /// Get an object
    async fn get(&self, bucket: &str, key: &str) -> Result<Bytes, StorageError>;

    /// Get with streaming (large files)
    async fn get_stream(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, StorageError>> + Send>>, StorageError>;

    /// Get a range of bytes
    async fn get_range(
        &self,
        bucket: &str,
        key: &str,
        start: u64,
        end: u64,
    ) -> Result<Bytes, StorageError>;

    /// Check if object exists
    async fn exists(&self, bucket: &str, key: &str) -> Result<bool, StorageError>;

    /// Get object metadata
    async fn head(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError>;

    /// Delete an object
    async fn delete(&self, bucket: &str, key: &str) -> Result<(), StorageError>;

    /// List objects with prefix
    async fn list(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError>;

    /// Create a bucket
    async fn create_bucket(&self, bucket: &str) -> Result<(), StorageError>;
}
```

**Implementations:**

| Implementation | Crate | Use Case |
|----------------|-------|----------|
| `S3Storage` | `aws-sdk-s3` | AWS, MinIO, any S3-compatible |
| `GarageStorage` | `aws-sdk-s3` | Homelab S3 (Garage) |
| `LocalStorage` | `tokio::fs` | Development, single-node |
| `InMemoryStorage` | built-in | Testing |

## 3. Cache Trait

Fast key-value lookups for hot data (secmaster, recent prices).

```rust
use async_trait::async_trait;
use bytes::Bytes;

/// Cache abstraction
#[async_trait]
pub trait Cache: Send + Sync {
    /// Get a value
    async fn get(&self, key: &str) -> Result<Option<Bytes>, CacheError>;

    /// Set a value with optional TTL
    async fn set(
        &self,
        key: &str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError>;

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

    /// Increment a counter
    async fn incr(&self, key: &str) -> Result<i64, CacheError>;

    /// Get multiple keys
    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<Bytes>>, CacheError>;

    /// Set multiple keys
    async fn mset(&self, pairs: &[(&str, Bytes)]) -> Result<(), CacheError>;

    /// Hash operations (for structured data)
    async fn hget(&self, key: &str, field: &str) -> Result<Option<Bytes>, CacheError>;
    async fn hset(&self, key: &str, field: &str, value: Bytes) -> Result<(), CacheError>;
    async fn hgetall(&self, key: &str) -> Result<HashMap<String, Bytes>, CacheError>;
}
```

**Implementations:**

| Implementation | Crate | Use Case |
|----------------|-------|----------|
| `RedisCache` | `redis` | Production, shared cache |
| `InMemoryCache` | `moka` | Single-node, development |
| `NoOpCache` | built-in | Disable caching |

**Note:** Redis pub/sub is NOT used - transport handles messaging.

## 4. Journal Trait

Append-only log for audit trail and change data capture.

```rust
use async_trait::async_trait;
use bytes::Bytes;

/// Journal entry
pub struct JournalEntry {
    pub sequence: u64,
    pub timestamp: u64,
    pub topic: String,
    pub key: Option<Bytes>,
    pub payload: Bytes,
    pub headers: HashMap<String, String>,
}

/// Journal reader for replay
#[async_trait]
pub trait JournalReader: Send + Sync {
    async fn next(&mut self) -> Result<Option<JournalEntry>, JournalError>;
    async fn seek(&mut self, position: JournalPosition) -> Result<(), JournalError>;
}

/// Journal abstraction
#[async_trait]
pub trait Journal: Send + Sync {
    /// Append an entry
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

    /// Get current end position
    async fn end_position(&self, topic: &str) -> Result<u64, JournalError>;

    /// Create a topic
    async fn create_topic(&self, config: TopicConfig) -> Result<(), JournalError>;
}

pub enum JournalPosition {
    Beginning,
    End,
    Sequence(u64),
    Time(u64),
}

pub struct TopicConfig {
    pub name: String,
    pub partitions: u32,
    pub retention: Duration,
    pub compaction: bool,  // Keep only latest per key
}
```

**Implementations:**

| Implementation | Crate | Use Case |
|----------------|-------|----------|
| `NatsJournal` | `async-nats` | JetStream as append-only log |
| `ChronicleJournal` | FFI | On-prem, high-performance |
| `FileJournal` | `tokio::fs` | Simple file-based for dev |
| `InMemoryJournal` | built-in | Testing |

## Factory Pattern

Runtime selection based on configuration:

```rust
use crate::config::MiddlewareConfig;

pub struct MiddlewareFactory;

impl MiddlewareFactory {
    pub async fn create_transport(config: &MiddlewareConfig) -> Result<Arc<dyn Transport>, Error> {
        match config.transport.type_.as_str() {
            "nats" => {
                let client = async_nats::connect(&config.transport.url).await?;
                Ok(Arc::new(NatsTransport::new(client)))
            }
            "aeron" => {
                let ctx = AeronContext::new(&config.transport.aeron)?;
                Ok(Arc::new(AeronTransport::new(ctx)))
            }
            "memory" => Ok(Arc::new(InMemoryTransport::new())),
            _ => Err(Error::UnknownTransport(config.transport.type_.clone())),
        }
    }

    pub async fn create_storage(config: &MiddlewareConfig) -> Result<Arc<dyn Storage>, Error> {
        match config.storage.type_.as_str() {
            "s3" => {
                let s3_config = aws_config::from_env()
                    .endpoint_url(&config.storage.endpoint)
                    .load()
                    .await;
                let client = aws_sdk_s3::Client::new(&s3_config);
                Ok(Arc::new(S3Storage::new(client)))
            }
            "local" => Ok(Arc::new(LocalStorage::new(&config.storage.path))),
            "memory" => Ok(Arc::new(InMemoryStorage::new())),
            _ => Err(Error::UnknownStorage(config.storage.type_.clone())),
        }
    }

    pub async fn create_cache(config: &MiddlewareConfig) -> Result<Arc<dyn Cache>, Error> {
        match config.cache.type_.as_str() {
            "redis" => {
                let client = redis::Client::open(&config.cache.url)?;
                Ok(Arc::new(RedisCache::new(client)))
            }
            "memory" => Ok(Arc::new(InMemoryCache::new(config.cache.max_size))),
            "none" => Ok(Arc::new(NoOpCache)),
            _ => Err(Error::UnknownCache(config.cache.type_.clone())),
        }
    }

    pub async fn create_journal(config: &MiddlewareConfig) -> Result<Arc<dyn Journal>, Error> {
        match config.journal.type_.as_str() {
            "nats" => {
                let client = async_nats::connect(&config.journal.url).await?;
                Ok(Arc::new(NatsJournal::new(client)))
            }
            "file" => Ok(Arc::new(FileJournal::new(&config.journal.path))),
            "memory" => Ok(Arc::new(InMemoryJournal::new())),
            _ => Err(Error::UnknownJournal(config.journal.type_.clone())),
        }
    }
}
```

## Environment Configuration

Middleware selection in environment YAML:

```yaml
# exchanges/environments/kalshi-prod.yaml
name: kalshi-prod
feed: kalshi
schema: trade:v1

transport:
  type: nats                           # nats | aeron | chronicle | memory
  url: nats://nats.ssmd.local:4222
  jetstream:
    enabled: true
    domain: ssmd

storage:
  type: s3                             # s3 | local | memory
  endpoint: http://garage.ssmd.local:3900
  region: garage
  buckets:
    raw: ssmd-raw
    normalized: ssmd-normalized

cache:
  type: redis                          # redis | memory | none
  url: redis://redis.ssmd.local:6379
  max_connections: 10

journal:
  type: nats                           # nats | chronicle | file | memory
  url: nats://nats.ssmd.local:4222
  topics:
    secmaster: ssmd.journal.secmaster
    audit: ssmd.journal.audit
```

## Testing Configuration

In-memory implementations for tests:

```yaml
# exchanges/environments/test.yaml
name: test
feed: kalshi-mock
schema: trade:v1

transport:
  type: memory

storage:
  type: memory

cache:
  type: memory

journal:
  type: memory
```

## Middleware Validation

CLI validates middleware configuration:

```bash
$ ssmd validate

Validating environments...
  ✓ kalshi-prod: transport nats reachable
  ✓ kalshi-prod: storage s3 endpoint reachable
  ✓ kalshi-prod: cache redis reachable
  ✓ kalshi-prod: journal topics exist

All validations passed.
```
