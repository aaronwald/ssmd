# Phase 2: Connector → NATS Streaming Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Connect the Kalshi WebSocket connector to NATS JetStream so market data flows to agents.

**Architecture:** Add `async-nats` crate to middleware, implement `NatsTransport` behind the existing `Transport` trait, update `MiddlewareFactory` to create NATS transports, and configure JetStream streams for market data persistence.

**Tech Stack:** Rust, async-nats 0.38, NATS JetStream, Cap'n Proto

---

## Task 1: Add async-nats Dependency

**Files:**
- Modify: `ssmd-rust/crates/middleware/Cargo.toml`

**Step 1: Add the dependency**

Add `async-nats` to workspace and middleware crate:

```toml
# In ssmd-rust/Cargo.toml [workspace.dependencies] section, add:
async-nats = "0.38"
```

```toml
# In ssmd-rust/crates/middleware/Cargo.toml [dependencies] section, add:
async-nats = { workspace = true }
```

**Step 2: Verify it compiles**

Run: `cd /workspaces/ssmd && make rust-build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add ssmd-rust/Cargo.toml ssmd-rust/crates/middleware/Cargo.toml
git commit -m "feat(middleware): add async-nats dependency"
```

---

## Task 2: Create NatsTransport Struct

**Files:**
- Create: `ssmd-rust/crates/middleware/src/nats/mod.rs`
- Create: `ssmd-rust/crates/middleware/src/nats/transport.rs`
- Modify: `ssmd-rust/crates/middleware/src/lib.rs`

**Step 1: Create nats module structure**

Create `ssmd-rust/crates/middleware/src/nats/mod.rs`:

```rust
mod transport;

pub use transport::NatsTransport;
```

**Step 2: Create NatsTransport with connection**

Create `ssmd-rust/crates/middleware/src/nats/transport.rs`:

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_nats::Client;
use async_trait::async_trait;
use bytes::Bytes;

use crate::error::TransportError;
use crate::latency::now_tsc;
use crate::transport::{Subscription, Transport, TransportMessage};

/// NATS transport implementation
pub struct NatsTransport {
    client: Client,
    sequence: AtomicU64,
}

impl NatsTransport {
    /// Create a new NatsTransport from an existing client
    pub fn new(client: Client) -> Self {
        Self {
            client,
            sequence: AtomicU64::new(0),
        }
    }

    /// Connect to NATS server and create transport
    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let client = async_nats::connect(url)
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        Ok(Self::new(client))
    }

    #[inline]
    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }
}
```

**Step 3: Export from lib.rs**

Add to `ssmd-rust/crates/middleware/src/lib.rs`:

```rust
pub mod nats;
pub use nats::NatsTransport;
```

**Step 4: Verify it compiles**

Run: `cd /workspaces/ssmd && make rust-build`
Expected: Build succeeds (transport trait not yet implemented)

**Step 5: Commit**

```bash
git add ssmd-rust/crates/middleware/src/nats ssmd-rust/crates/middleware/src/lib.rs
git commit -m "feat(middleware): add NatsTransport struct with connection"
```

---

## Task 3: Implement Transport Trait for NatsTransport

**Files:**
- Modify: `ssmd-rust/crates/middleware/src/nats/transport.rs`

**Step 1: Write failing test for publish**

Add to `ssmd-rust/crates/middleware/src/nats/transport.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running NATS server
    // Run: docker run -p 4222:4222 nats:latest

    #[tokio::test]
    #[ignore] // Requires NATS server
    async fn test_publish_succeeds() {
        let transport = NatsTransport::connect("nats://localhost:4222").await.unwrap();
        let result = transport.publish("test.subject", Bytes::from("hello")).await;
        assert!(result.is_ok());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware --lib nats -- --ignored 2>&1 | head -20`
Expected: FAIL - Transport trait not implemented

**Step 3: Implement publish methods**

Add to `NatsTransport` impl in `transport.rs`:

```rust
#[async_trait]
impl Transport for NatsTransport {
    async fn publish(&self, subject: &str, payload: Bytes) -> Result<(), TransportError> {
        self.client
            .publish(subject.to_string(), payload)
            .await
            .map_err(|e| TransportError::PublishFailed(e.to_string()))
    }

    async fn publish_with_headers(
        &self,
        subject: &str,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<(), TransportError> {
        let mut nats_headers = async_nats::HeaderMap::new();
        for (k, v) in headers {
            nats_headers.insert(
                k.as_str().parse().map_err(|_| TransportError::PublishFailed("invalid header name".to_string()))?,
                v.as_str().parse().map_err(|_| TransportError::PublishFailed("invalid header value".to_string()))?,
            );
        }

        self.client
            .publish_with_headers(subject.to_string(), nats_headers, payload)
            .await
            .map_err(|e| TransportError::PublishFailed(e.to_string()))
    }

    async fn subscribe(&self, subject: &str) -> Result<Box<dyn Subscription>, TransportError> {
        let subscriber = self.client
            .subscribe(subject.to_string())
            .await
            .map_err(|e| TransportError::SubscribeFailed(e.to_string()))?;
        Ok(Box::new(NatsSubscription::new(subscriber)))
    }

    async fn request(
        &self,
        subject: &str,
        payload: Bytes,
        timeout: Duration,
    ) -> Result<TransportMessage, TransportError> {
        let response = tokio::time::timeout(
            timeout,
            self.client.request(subject.to_string(), payload),
        )
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(|e| TransportError::RequestFailed(e.to_string()))?;

        Ok(TransportMessage {
            subject: response.subject.to_string(),
            payload: response.payload,
            headers: HashMap::new(),
            timestamp: now_tsc(),
            sequence: None,
        })
    }
}
```

**Step 4: Add NatsSubscription implementation**

Add above the `impl Transport`:

```rust
struct NatsSubscription {
    subscriber: async_nats::Subscriber,
}

impl NatsSubscription {
    fn new(subscriber: async_nats::Subscriber) -> Self {
        Self { subscriber }
    }
}

#[async_trait]
impl Subscription for NatsSubscription {
    async fn next(&mut self) -> Result<TransportMessage, TransportError> {
        let msg = self.subscriber
            .next()
            .await
            .ok_or_else(|| TransportError::SubscribeFailed("subscription closed".to_string()))?;

        Ok(TransportMessage {
            subject: msg.subject.to_string(),
            payload: msg.payload,
            headers: HashMap::new(),
            timestamp: now_tsc(),
            sequence: None,
        })
    }

    async fn ack(&self, _sequence: u64) -> Result<(), TransportError> {
        // Core NATS doesn't have ack - JetStream does
        Ok(())
    }

    async fn unsubscribe(self: Box<Self>) -> Result<(), TransportError> {
        self.subscriber
            .unsubscribe()
            .await
            .map_err(|e| TransportError::SubscribeFailed(e.to_string()))
    }
}
```

**Step 5: Add required imports**

Add at top of file:

```rust
use futures_util::StreamExt;
```

And add `futures-util` to middleware Cargo.toml dependencies.

**Step 6: Run tests**

Run: `cd /workspaces/ssmd && make rust-test`
Expected: All existing tests pass

**Step 7: Commit**

```bash
git add ssmd-rust/crates/middleware/
git commit -m "feat(middleware): implement Transport trait for NatsTransport"
```

---

## Task 4: Add TransportError Variants

**Files:**
- Modify: `ssmd-rust/crates/middleware/src/error.rs`

**Step 1: Check current error variants**

Read the file to see what's there.

**Step 2: Add missing variants if needed**

Ensure these variants exist in `TransportError`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("publish failed: {0}")]
    PublishFailed(String),
    #[error("subscribe failed: {0}")]
    SubscribeFailed(String),
    #[error("request failed: {0}")]
    RequestFailed(String),
    #[error("timeout")]
    Timeout,
}
```

**Step 3: Verify build**

Run: `cd /workspaces/ssmd && make rust-build`
Expected: Build succeeds

**Step 4: Commit if changes made**

```bash
git add ssmd-rust/crates/middleware/src/error.rs
git commit -m "feat(middleware): add TransportError variants for NATS"
```

---

## Task 5: Update MiddlewareFactory for NATS

**Files:**
- Modify: `ssmd-rust/crates/middleware/src/factory.rs`

**Step 1: Write failing test**

Add to tests in `factory.rs`:

```rust
#[tokio::test]
#[ignore] // Requires NATS server
async fn test_create_nats_transport() {
    let mut env = make_test_env();
    env.transport.transport_type = TransportType::Nats;
    env.transport.url = Some("nats://localhost:4222".to_string());

    let transport = MiddlewareFactory::create_transport(&env).await.unwrap();
    drop(transport);
}
```

**Step 2: Run test to verify it fails**

Run: `cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware factory::tests::test_create_nats_transport -- --ignored 2>&1`
Expected: FAIL - returns UnsupportedTransport

**Step 3: Make factory async and add NATS support**

Update `factory.rs`:

```rust
use crate::nats::NatsTransport;

impl MiddlewareFactory {
    /// Create a transport based on environment configuration
    pub async fn create_transport(env: &Environment) -> Result<Arc<dyn Transport>, FactoryError> {
        match env.transport.transport_type {
            TransportType::Memory => Ok(Arc::new(InMemoryTransport::new())),
            TransportType::Nats => {
                let url = env.transport.url.as_ref()
                    .ok_or_else(|| FactoryError::ConfigError("NATS URL required".to_string()))?;
                let transport = NatsTransport::connect(url)
                    .await
                    .map_err(|e| FactoryError::ConfigError(e.to_string()))?;
                Ok(Arc::new(transport))
            }
            TransportType::Mqtt => {
                Err(FactoryError::UnsupportedTransport(TransportType::Mqtt))
            }
        }
    }

    // ... rest of methods unchanged
}
```

**Step 4: Update existing sync tests to use block_on or make async**

Update tests that call `create_transport` to be async:

```rust
#[tokio::test]
async fn test_create_memory_transport() {
    let env = make_test_env();
    let transport = MiddlewareFactory::create_transport(&env).await.unwrap();
    drop(transport);
}
```

**Step 5: Run all tests**

Run: `cd /workspaces/ssmd && make rust-test`
Expected: All tests pass

**Step 6: Commit**

```bash
git add ssmd-rust/crates/middleware/src/factory.rs
git commit -m "feat(middleware): add NATS transport to MiddlewareFactory"
```

---

## Task 6: Add JetStream Stream Creation

**Files:**
- Modify: `ssmd-rust/crates/middleware/src/nats/transport.rs`

**Step 1: Add JetStream context to NatsTransport**

Update the struct and constructor:

```rust
use async_nats::jetstream::{self, Context};

pub struct NatsTransport {
    client: Client,
    jetstream: Context,
    sequence: AtomicU64,
}

impl NatsTransport {
    pub fn new(client: Client) -> Self {
        let jetstream = jetstream::new(client.clone());
        Self {
            client,
            jetstream,
            sequence: AtomicU64::new(0),
        }
    }

    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let client = async_nats::connect(url)
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        Ok(Self::new(client))
    }

    /// Get JetStream context for stream operations
    pub fn jetstream(&self) -> &Context {
        &self.jetstream
    }
}
```

**Step 2: Add stream creation helper**

Add method to `NatsTransport`:

```rust
use async_nats::jetstream::stream::{Config, RetentionPolicy, StorageType};

impl NatsTransport {
    /// Create or get a JetStream stream for market data
    pub async fn ensure_stream(
        &self,
        stream_name: &str,
        subjects: Vec<String>,
    ) -> Result<(), TransportError> {
        let config = Config {
            name: stream_name.to_string(),
            subjects,
            retention: RetentionPolicy::Limits,
            storage: StorageType::File,
            max_age: std::time::Duration::from_secs(24 * 60 * 60), // 24 hours
            ..Default::default()
        };

        self.jetstream
            .get_or_create_stream(config)
            .await
            .map_err(|e| TransportError::PublishFailed(format!("stream creation failed: {}", e)))?;

        Ok(())
    }
}
```

**Step 3: Add test**

```rust
#[tokio::test]
#[ignore] // Requires NATS server with JetStream
async fn test_ensure_stream() {
    let transport = NatsTransport::connect("nats://localhost:4222").await.unwrap();
    let result = transport.ensure_stream(
        "TEST_STREAM",
        vec!["test.>".to_string()],
    ).await;
    assert!(result.is_ok());
}
```

**Step 4: Run tests**

Run: `cd /workspaces/ssmd && make rust-test`
Expected: All tests pass

**Step 5: Commit**

```bash
git add ssmd-rust/crates/middleware/src/nats/
git commit -m "feat(middleware): add JetStream stream creation to NatsTransport"
```

---

## Task 7: Add Environment Subject Prefix Helper

**Files:**
- Create: `ssmd-rust/crates/middleware/src/nats/subjects.rs`
- Modify: `ssmd-rust/crates/middleware/src/nats/mod.rs`

**Step 1: Write test for subject formatting**

Create `ssmd-rust/crates/middleware/src/nats/subjects.rs`:

```rust
/// Helper for NATS subject formatting with environment prefix
pub struct SubjectBuilder {
    env: String,
    feed: String,
}

impl SubjectBuilder {
    pub fn new(env: impl Into<String>, feed: impl Into<String>) -> Self {
        Self {
            env: env.into(),
            feed: feed.into(),
        }
    }

    /// Build subject for trade messages: {env}.{feed}.trade.{ticker}
    pub fn trade(&self, ticker: &str) -> String {
        format!("{}.{}.trade.{}", self.env, self.feed, ticker)
    }

    /// Build subject for orderbook messages: {env}.{feed}.orderbook.{ticker}
    pub fn orderbook(&self, ticker: &str) -> String {
        format!("{}.{}.orderbook.{}", self.env, self.feed, ticker)
    }

    /// Build wildcard subject for all feed data: {env}.{feed}.>
    pub fn all(&self) -> String {
        format!("{}.{}.>", self.env, self.feed)
    }

    /// Build stream name: {ENV}_{FEED} (uppercase)
    pub fn stream_name(&self) -> String {
        format!("{}_{}", self.env.to_uppercase(), self.feed.to_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trade_subject() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.trade("BTCUSD"), "kalshi-dev.kalshi.trade.BTCUSD");
    }

    #[test]
    fn test_orderbook_subject() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.orderbook("BTCUSD"), "kalshi-dev.kalshi.orderbook.BTCUSD");
    }

    #[test]
    fn test_wildcard_subject() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.all(), "kalshi-dev.kalshi.>");
    }

    #[test]
    fn test_stream_name() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.stream_name(), "KALSHI-DEV_KALSHI");
    }
}
```

**Step 2: Export from mod.rs**

Update `ssmd-rust/crates/middleware/src/nats/mod.rs`:

```rust
mod subjects;
mod transport;

pub use subjects::SubjectBuilder;
pub use transport::NatsTransport;
```

**Step 3: Export from lib.rs**

Update `ssmd-rust/crates/middleware/src/lib.rs` to also export:

```rust
pub use nats::{NatsTransport, SubjectBuilder};
```

**Step 4: Run tests**

Run: `cd /workspaces/ssmd && make rust-test`
Expected: All tests pass including new subject tests

**Step 5: Commit**

```bash
git add ssmd-rust/crates/middleware/src/nats/
git commit -m "feat(middleware): add SubjectBuilder for NATS subject formatting"
```

---

## Task 8: Integration Test with Docker NATS

**Files:**
- Create: `ssmd-rust/crates/middleware/tests/nats_integration.rs`

**Step 1: Create integration test file**

```rust
//! Integration tests for NATS transport
//!
//! Run with: cargo test -p ssmd-middleware --test nats_integration -- --ignored
//! Requires: docker run -p 4222:4222 nats:latest -js

use bytes::Bytes;
use ssmd_middleware::{NatsTransport, SubjectBuilder, Transport};

#[tokio::test]
#[ignore]
async fn test_nats_publish_subscribe_roundtrip() {
    let transport = NatsTransport::connect("nats://localhost:4222")
        .await
        .expect("Failed to connect to NATS");

    let subjects = SubjectBuilder::new("test-env", "kalshi");

    // Subscribe first
    let mut sub = transport
        .subscribe(&subjects.trade("BTCUSD"))
        .await
        .expect("Failed to subscribe");

    // Publish
    transport
        .publish(&subjects.trade("BTCUSD"), Bytes::from("test message"))
        .await
        .expect("Failed to publish");

    // Receive
    let msg = sub.next().await.expect("Failed to receive");
    assert_eq!(msg.payload, Bytes::from("test message"));
}

#[tokio::test]
#[ignore]
async fn test_jetstream_stream_creation() {
    let transport = NatsTransport::connect("nats://localhost:4222")
        .await
        .expect("Failed to connect to NATS");

    let subjects = SubjectBuilder::new("test-env", "kalshi");

    transport
        .ensure_stream(&subjects.stream_name(), vec![subjects.all()])
        .await
        .expect("Failed to create stream");
}
```

**Step 2: Run integration tests (requires NATS)**

Run: `docker run -d --name nats-test -p 4222:4222 nats:latest -js && sleep 2 && cd /workspaces/ssmd/ssmd-rust && cargo test -p ssmd-middleware --test nats_integration -- --ignored; docker rm -f nats-test`

Expected: Tests pass

**Step 3: Commit**

```bash
git add ssmd-rust/crates/middleware/tests/
git commit -m "test(middleware): add NATS integration tests"
```

---

## Task 9: Update Connector Publisher to Use Transport

**Files:**
- Modify: `ssmd-rust/crates/connector/src/publisher.rs`

**Step 1: Verify publisher already uses Transport trait**

The publisher at `ssmd-rust/crates/connector/src/publisher.rs` already accepts `Arc<dyn Transport>` and uses the `SubjectBuilder` pattern (env_prefix + feed_name).

**Step 2: Add orderbook publishing support**

Add to `publisher.rs`:

```rust
use ssmd_schema::{orderbook_update, level};

/// Order book level data
#[derive(Debug, Clone)]
pub struct LevelData {
    pub price: f64,
    pub size: u32,
}

/// Order book update data for publishing
#[derive(Debug, Clone)]
pub struct OrderBookData {
    pub timestamp_nanos: u64,
    pub ticker: String,
    pub bids: Vec<LevelData>,
    pub asks: Vec<LevelData>,
}

impl Publisher {
    /// Publish an orderbook update to the transport
    pub async fn publish_orderbook(&self, book: &OrderBookData) -> Result<(), TransportError> {
        let mut message = Builder::new_default();
        {
            let mut book_builder = message.init_root::<orderbook_update::Builder>();
            book_builder.set_timestamp(book.timestamp_nanos);
            book_builder.set_ticker(&book.ticker);

            let mut bids = book_builder.reborrow().init_bids(book.bids.len() as u32);
            for (i, bid) in book.bids.iter().enumerate() {
                let mut level_builder = bids.reborrow().get(i as u32);
                level_builder.set_price(bid.price);
                level_builder.set_size(bid.size);
            }

            let mut asks = book_builder.reborrow().init_asks(book.asks.len() as u32);
            for (i, ask) in book.asks.iter().enumerate() {
                let mut level_builder = asks.reborrow().get(i as u32);
                level_builder.set_price(ask.price);
                level_builder.set_size(ask.size);
            }
        }

        let mut output = Vec::new();
        capnp::serialize::write_message(&mut output, &message)
            .map_err(|e| TransportError::PublishFailed(e.to_string()))?;

        let subject = format!(
            "{}.{}.orderbook.{}",
            self.env_prefix, self.feed_name, book.ticker
        );
        self.transport.publish(&subject, Bytes::from(output)).await
    }
}
```

**Step 3: Add test for orderbook publishing**

```rust
#[tokio::test]
async fn test_publish_orderbook() {
    let transport = Arc::new(InMemoryTransport::new());
    let publisher = Publisher::new(transport.clone(), "kalshi-dev", "kalshi");

    let mut sub = transport.subscribe("kalshi-dev.kalshi.orderbook.BTCUSD").await.unwrap();

    let book = OrderBookData {
        timestamp_nanos: 1703318400000000000,
        ticker: "BTCUSD".to_string(),
        bids: vec![LevelData { price: 100.0, size: 10 }],
        asks: vec![LevelData { price: 101.0, size: 5 }],
    };

    publisher.publish_orderbook(&book).await.unwrap();

    let msg = sub.next().await.unwrap();
    assert_eq!(msg.subject, "kalshi-dev.kalshi.orderbook.BTCUSD");
    assert!(!msg.payload.is_empty());
}
```

**Step 4: Run tests**

Run: `cd /workspaces/ssmd && make rust-test`
Expected: All tests pass

**Step 5: Commit**

```bash
git add ssmd-rust/crates/connector/src/publisher.rs
git commit -m "feat(connector): add orderbook publishing to Publisher"
```

---

## Task 10: Final Validation

**Files:** None (validation only)

**Step 1: Run full test suite**

Run: `cd /workspaces/ssmd && make all-test`
Expected: All Go and Rust tests pass

**Step 2: Run lints**

Run: `cd /workspaces/ssmd && make all-lint`
Expected: No lint errors

**Step 3: Build release**

Run: `cd /workspaces/ssmd && make all-build`
Expected: Build succeeds

**Step 4: Commit any final fixes**

If any fixes needed, commit them.

**Step 5: Create PR or merge**

```bash
git push -u origin feature/phase2-nats-streaming
```

---

## Summary

After completing all tasks, the codebase will have:

1. **NatsTransport** - Full `Transport` trait implementation using `async-nats`
2. **JetStream support** - `ensure_stream()` for creating persistent streams
3. **SubjectBuilder** - Helper for consistent subject formatting with env prefix
4. **Updated MiddlewareFactory** - Creates NATS transports from config
5. **Publisher with orderbook support** - Publishes both trades and orderbooks

The connector can now publish Cap'n Proto encoded market data to NATS JetStream, enabling the Agent Pipeline (Phase 3) to consume it.
