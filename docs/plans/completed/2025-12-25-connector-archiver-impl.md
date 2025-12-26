# Connector NATS-Only & Archiver Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor connector to NATS-only output with raw JSON passthrough, and create archiver service to persist data from NATS to disk.

**Architecture:** Connector publishes raw JSON to NATS subjects `{env}.{feed}.json.{type}.{ticker}`. Archiver subscribes to NATS JetStream and writes JSONL.gz files with 15-minute rotation. Manifest tracks files, sequences, and gaps.

**Tech Stack:** Rust, async-nats, tokio, flate2 (gzip), serde_json, clap

---

## Phase 1: Connector Refactor

### Task 1: Add JSON Subject Methods to SubjectBuilder

**Files:**
- Modify: `ssmd-rust/crates/middleware/src/nats/subjects.rs`

**Step 1: Write the failing tests**

Add to the existing test module at the bottom of `subjects.rs`:

```rust
#[test]
fn test_json_trade_subject() {
    let builder = SubjectBuilder::new("prod", "kalshi");
    assert_eq!(builder.json_trade("INXD-25001"), "prod.kalshi.json.trade.INXD-25001");
}

#[test]
fn test_json_ticker_subject() {
    let builder = SubjectBuilder::new("prod", "kalshi");
    assert_eq!(builder.json_ticker("KXBTC-25001"), "prod.kalshi.json.ticker.KXBTC-25001");
}

#[test]
fn test_json_orderbook_subject() {
    let builder = SubjectBuilder::new("prod", "kalshi");
    assert_eq!(builder.json_orderbook("INXD-25001"), "prod.kalshi.json.orderbook.INXD-25001");
}
```

**Step 2: Run tests to verify they fail**

Run: `cd ssmd-rust && cargo test -p ssmd-middleware json_`
Expected: FAIL - methods not found

**Step 3: Add the json_* methods**

Add these methods to the `impl SubjectBuilder` block (after the existing `ticker` method):

```rust
/// Build subject for JSON trade messages: {env}.{feed}.json.trade.{ticker}
/// Not cached - allocates each call (acceptable for MVP volume)
pub fn json_trade(&self, ticker: &str) -> String {
    format!("{}.json.trade.{}", &self.wildcard[..self.wildcard.len()-1], ticker)
}

/// Build subject for JSON ticker messages: {env}.{feed}.json.ticker.{ticker}
pub fn json_ticker(&self, ticker: &str) -> String {
    format!("{}.json.ticker.{}", &self.wildcard[..self.wildcard.len()-1], ticker)
}

/// Build subject for JSON orderbook messages: {env}.{feed}.json.orderbook.{ticker}
pub fn json_orderbook(&self, ticker: &str) -> String {
    format!("{}.json.orderbook.{}", &self.wildcard[..self.wildcard.len()-1], ticker)
}
```

Wait - the wildcard ends with `>`, so we can't just slice. Let's use a simpler approach:

```rust
/// Build subject for JSON trade messages: {env}.{feed}.json.trade.{ticker}
pub fn json_trade(&self, ticker: &str) -> String {
    // trade_prefix is "{env}.{feed}.trade." - replace "trade" with "json.trade"
    let base = &self.trade_prefix[..self.trade_prefix.len() - 7]; // remove "trade."
    format!("{}json.trade.{}", base, ticker)
}

/// Build subject for JSON ticker messages: {env}.{feed}.json.ticker.{ticker}
pub fn json_ticker(&self, ticker: &str) -> String {
    let base = &self.ticker_prefix[..self.ticker_prefix.len() - 8]; // remove "ticker."
    format!("{}json.ticker.{}", base, ticker)
}

/// Build subject for JSON orderbook messages: {env}.{feed}.json.orderbook.{ticker}
pub fn json_orderbook(&self, ticker: &str) -> String {
    let base = &self.trade_prefix[..self.trade_prefix.len() - 7]; // remove "trade."
    format!("{}json.orderbook.{}", base, ticker)
}
```

Actually, let's just add a base prefix for simplicity. Add a new field to SubjectBuilder:

In struct definition, add:
```rust
/// Pre-computed prefix: "{env}.{feed}."
base_prefix: Arc<str>,
```

In `new()`, add:
```rust
let base_prefix: Arc<str> = format!("{}.", env, feed).into();
```

Wait, that's wrong. Let me just do it simply:

```rust
/// Build subject for JSON trade messages: {env}.{feed}.json.trade.{ticker}
pub fn json_trade(&self, ticker: &str) -> String {
    // Extract env.feed from trade_prefix which is "{env}.{feed}.trade."
    let prefix_len = self.trade_prefix.len() - 6; // "trade." is 6 chars
    format!("{}json.trade.{}", &self.trade_prefix[..prefix_len], ticker)
}

/// Build subject for JSON ticker messages: {env}.{feed}.json.ticker.{ticker}
pub fn json_ticker(&self, ticker: &str) -> String {
    let prefix_len = self.ticker_prefix.len() - 7; // "ticker." is 7 chars
    format!("{}json.ticker.{}", &self.ticker_prefix[..prefix_len], ticker)
}

/// Build subject for JSON orderbook messages: {env}.{feed}.json.orderbook.{ticker}
pub fn json_orderbook(&self, ticker: &str) -> String {
    let prefix_len = self.trade_prefix.len() - 6; // "trade." is 6 chars
    format!("{}json.orderbook.{}", &self.trade_prefix[..prefix_len], ticker)
}
```

**Step 4: Run tests to verify they pass**

Run: `cd ssmd-rust && cargo test -p ssmd-middleware json_`
Expected: PASS

**Step 5: Run all middleware tests**

Run: `cd ssmd-rust && cargo test -p ssmd-middleware`
Expected: All tests PASS

**Step 6: Commit**

```bash
git add ssmd-rust/crates/middleware/src/nats/subjects.rs
git commit -m "feat(middleware): add JSON subject methods to SubjectBuilder"
```

---

### Task 2: Simplify NatsWriter to Raw JSON Passthrough

**Files:**
- Modify: `ssmd-rust/crates/connector/src/nats_writer.rs`

**Step 1: Update the imports and remove Cap'n Proto dependencies**

Replace the imports section:

```rust
//! NATS Writer - publishes raw JSON messages to NATS
//!
//! Passes through incoming JSON messages from connectors directly to NATS.
//! No transformation - raw bytes are preserved for archiving.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use tracing::{trace, warn};

use ssmd_middleware::{SubjectBuilder, Transport, TransportError};

use crate::error::WriterError;
use crate::kalshi::messages::WsMessage;
use crate::message::Message;
use crate::traits::Writer;
```

**Step 2: Simplify the NatsWriter struct**

Replace the struct and impl:

```rust
/// Writer that publishes raw JSON messages to NATS
pub struct NatsWriter {
    transport: Arc<dyn Transport>,
    subjects: SubjectBuilder,
    message_count: u64,
}

impl NatsWriter {
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<String>,
        feed_name: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::new(env_name, feed_name),
            message_count: 0,
        }
    }

    /// Get count of published messages
    pub fn message_count(&self) -> u64 {
        self.message_count
    }
}
```

**Step 3: Simplify the Writer impl to raw passthrough**

Replace the Writer impl:

```rust
#[async_trait]
impl Writer for NatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        // Parse just enough to extract message type and ticker
        let ws_msg: WsMessage = match serde_json::from_slice(&msg.data) {
            Ok(m) => m,
            Err(e) => {
                trace!(error = %e, "Failed to parse message, skipping");
                return Ok(()); // Skip unparseable messages
            }
        };

        let subject = match &ws_msg {
            WsMessage::Trade { msg: trade_data } => {
                self.subjects.json_trade(&trade_data.market_ticker)
            }
            WsMessage::Ticker { msg: ticker_data } => {
                self.subjects.json_ticker(&ticker_data.market_ticker)
            }
            WsMessage::OrderbookSnapshot { msg: ob_data } => {
                self.subjects.json_orderbook(&ob_data.market_ticker)
            }
            WsMessage::OrderbookDelta { msg: ob_data } => {
                self.subjects.json_orderbook(&ob_data.market_ticker)
            }
            WsMessage::Subscribed { .. } | WsMessage::Unsubscribed { .. } => {
                // Control messages, don't publish
                return Ok(());
            }
            WsMessage::Unknown => {
                warn!("Unknown message type received");
                return Ok(());
            }
        };

        // Publish raw bytes - no transformation
        self.transport
            .publish(&subject, Bytes::from(msg.data.clone()))
            .await
            .map_err(|e| WriterError::WriteFailed(format!("NATS publish failed: {}", e)))?;

        self.message_count += 1;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), WriterError> {
        trace!(messages = self.message_count, "NatsWriter closing");
        Ok(())
    }
}
```

**Step 4: Update the tests**

Replace the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_middleware::InMemoryTransport;

    #[tokio::test]
    async fn test_publish_trade_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        // Subscribe to exact subject
        let mut sub = transport
            .subscribe("dev.kalshi.json.trade.KXTEST-123")
            .await
            .unwrap();

        let trade_json = br#"{"type":"trade","sid":2,"seq":1,"msg":{"market_ticker":"KXTEST-123","price":50,"count":10,"side":"yes","ts":1732579880}}"#;
        let msg = Message::new("kalshi", trade_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.kalshi.json.trade.KXTEST-123");
        // Raw JSON preserved
        assert_eq!(received.payload.as_ref(), trade_json);
    }

    #[tokio::test]
    async fn test_publish_ticker_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        let mut sub = transport
            .subscribe("dev.kalshi.json.ticker.KXTEST-456")
            .await
            .unwrap();

        let ticker_json = br#"{"type":"ticker","sid":1,"msg":{"market_ticker":"KXTEST-456","yes_bid":45,"yes_ask":46,"price":45,"volume":1000,"open_interest":500,"ts":1732579880}}"#;
        let msg = Message::new("kalshi", ticker_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.kalshi.json.ticker.KXTEST-456");
        assert_eq!(received.payload.as_ref(), ticker_json);
    }

    #[tokio::test]
    async fn test_skip_control_messages() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        let subscribed_json = br#"{"type":"subscribed","id":1}"#;
        let msg = Message::new("kalshi", subscribed_json.to_vec());

        // Should not error
        writer.write(&msg).await.unwrap();

        // No messages published
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_message_count() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = NatsWriter::new(transport.clone(), "dev", "kalshi");

        // Need to subscribe to receive
        let _sub = transport
            .subscribe("dev.kalshi.json.trade.KXTEST-123")
            .await
            .unwrap();

        let trade_json = br#"{"type":"trade","sid":2,"seq":1,"msg":{"market_ticker":"KXTEST-123","price":50,"count":10,"side":"yes","ts":1732579880}}"#;
        let msg = Message::new("kalshi", trade_json.to_vec());

        writer.write(&msg).await.unwrap();
        writer.write(&msg).await.unwrap();

        assert_eq!(writer.message_count(), 2);
    }
}
```

**Step 5: Run tests**

Run: `cd ssmd-rust && cargo test -p ssmd-connector nats_writer`
Expected: All tests PASS

**Step 6: Commit**

```bash
git add ssmd-rust/crates/connector/src/nats_writer.rs
git commit -m "feat(connector): simplify NatsWriter to raw JSON passthrough"
```

---

### Task 3: Remove FileWriter and Update Exports

**Files:**
- Modify: `ssmd-rust/crates/connector/src/lib.rs`
- Delete: `ssmd-rust/crates/connector/src/writer.rs` (keep for now, just remove export)

**Step 1: Update lib.rs exports**

Remove `pub mod writer;` and `pub use writer::FileWriter;` from lib.rs.

Change:
```rust
pub mod writer;
```
to:
```rust
// writer.rs kept for backward compat but not exported
// TODO: Delete in next major version
mod writer;
```

Remove from the pub use section:
```rust
pub use writer::FileWriter;
```

**Step 2: Verify compilation**

Run: `cd ssmd-rust && cargo check -p ssmd-connector`
Expected: Should compile (FileWriter still exists, just not exported)

**Step 3: Commit**

```bash
git add ssmd-rust/crates/connector/src/lib.rs
git commit -m "refactor(connector): hide FileWriter export (NATS-only output)"
```

---

### Task 4: Update main.rs to NATS-Only

**Files:**
- Modify: `ssmd-rust/crates/ssmd-connector/src/main.rs`

**Step 1: Remove FileWriter import**

Change the import:
```rust
use ssmd_connector_lib::{
    kalshi::{KalshiConfig, KalshiConnector, KalshiCredentials},
    EnvResolver, FileWriter, KeyResolver, NatsWriter, Runner, ServerState, WebSocketConnector,
};
```
to:
```rust
use ssmd_connector_lib::{
    kalshi::{KalshiConfig, KalshiConnector, KalshiCredentials},
    EnvResolver, KeyResolver, NatsWriter, Runner, ServerState, WebSocketConnector,
};
```

**Step 2: Update run_kalshi_connector to require NATS**

Replace the `run_kalshi_connector` function:

```rust
/// Run Kalshi-specific connector with RSA authentication
async fn run_kalshi_connector(
    feed: &Feed,
    env_config: &Environment,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load Kalshi config from environment
    let config = KalshiConfig::from_env().map_err(|e| {
        error!(error = %e, "Failed to load Kalshi config");
        e
    })?;

    let credentials = KalshiCredentials::new(config.api_key, &config.private_key_pem).map_err(|e| {
        error!(error = %e, "Failed to create Kalshi credentials");
        e
    })?;

    info!(use_demo = config.use_demo, "Creating Kalshi connector");

    let connector = KalshiConnector::new(credentials, config.use_demo);

    // NATS transport required
    match env_config.transport.transport_type {
        TransportType::Nats => {
            info!(transport = "nats", "Using NATS writer (raw JSON)");
            let transport = MiddlewareFactory::create_transport(env_config).await?;
            let writer = NatsWriter::new(transport, &env_config.name, &feed.name);
            run_with_writer(feed, connector, writer, health_addr, shutdown_rx).await
        }
        TransportType::Memory => {
            error!("Memory transport not supported - use NATS transport");
            error!("Set transport.transport_type: nats in environment config");
            Err("Memory transport not supported - connector requires NATS".into())
        }
        TransportType::Mqtt => {
            error!("MQTT transport not yet supported");
            Err("MQTT transport not yet supported".into())
        }
    }
}
```

**Step 3: Update run_generic_connector to require NATS**

Replace the `run_generic_connector` function:

```rust
/// Run generic WebSocket connector
async fn run_generic_connector(
    feed: &Feed,
    env_config: &Environment,
    _output_path: &PathBuf,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get latest version
    let version = feed.get_latest_version().ok_or("No feed versions defined")?;

    // Determine connection URL based on feed type
    let url = match feed.feed_type {
        FeedType::Websocket => version.endpoint.clone(),
        FeedType::Rest => {
            error!("REST feeds not yet supported");
            return Err("REST feeds not yet supported".into());
        }
        FeedType::Multicast => {
            error!("Multicast feeds not yet supported");
            return Err("Multicast feeds not yet supported".into());
        }
    };

    // Resolve credentials from environment config
    let creds: Option<HashMap<String, String>> = if let Some(ref keys) = env_config.keys {
        let api_key_spec = keys.values().find(|k| k.key_type == KeyType::ApiKey);

        if let Some(key_spec) = api_key_spec {
            if let Some(ref source) = key_spec.source {
                let resolver = EnvResolver::new();
                match resolver.resolve(source) {
                    Ok(resolved_keys) => Some(resolved_keys),
                    Err(e) => {
                        error!(error = %e, "Failed to resolve credentials");
                        return Err(e.into());
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // NATS transport required
    match env_config.transport.transport_type {
        TransportType::Nats => {
            info!(transport = "nats", "Using NATS writer (raw JSON)");
            let transport = MiddlewareFactory::create_transport(env_config).await?;
            let connector = WebSocketConnector::new(&url, creds);
            let writer = NatsWriter::new(transport, &env_config.name, &feed.name);
            run_with_writer(feed, connector, writer, health_addr, shutdown_rx).await
        }
        TransportType::Memory => {
            error!("Memory transport not supported - use NATS transport");
            Err("Memory transport not supported - connector requires NATS".into())
        }
        TransportType::Mqtt => {
            error!("MQTT transport not yet supported");
            Err("MQTT transport not yet supported".into())
        }
    }
}
```

**Step 4: Remove unused output_path from main**

In `main()`, the output_path variable is no longer needed. Remove these lines:
```rust
    // Get output path from environment storage config
    let output_path = env_config
        .storage
        .path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"));
```

And update the generic connector call:
```rust
        _ => {
            run_generic_connector(&feed, &env_config, health_addr, shutdown_rx).await
        }
```

Update the function signature:
```rust
async fn run_generic_connector(
    feed: &Feed,
    env_config: &Environment,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
```

**Step 5: Update the module doc comment**

Change:
```rust
//! Connects to market data sources and writes to configured destinations.
//! Supports file (raw JSON) and NATS (Cap'n Proto) output modes.
```
to:
```rust
//! Connects to market data sources and publishes to NATS.
//! Raw JSON passthrough - no transformation.
```

**Step 6: Verify compilation**

Run: `cd ssmd-rust && cargo build -p ssmd-connector`
Expected: Success

**Step 7: Run all tests**

Run: `cd ssmd-rust && cargo test`
Expected: All tests PASS

**Step 8: Commit**

```bash
git add ssmd-rust/crates/ssmd-connector/src/main.rs
git commit -m "feat(connector): require NATS transport, remove file writer path

BREAKING CHANGE: Connector now requires NATS transport. File writing
moved to separate archiver service."
```

---

### Task 5: Update Integration Tests

**Files:**
- Modify: `ssmd-rust/crates/connector/src/lib.rs` (integration tests section)

**Step 1: Update integration tests to use NatsWriter**

The integration tests in lib.rs use FileWriter and DiskFlusher. Since we're keeping the file for backward compat but deprecating it, we can either:
1. Keep the tests as-is (they test the ring buffer / flusher, not the writer abstraction)
2. Mark them as deprecated

For now, keep them - they test the ring buffer which is still useful.

**Step 2: Run all tests to ensure nothing is broken**

Run: `cd ssmd-rust && cargo test`
Expected: All tests PASS

**Step 3: Commit if any changes**

No commit needed if no changes.

---

### Task 6: Update CLAUDE.md Documentation

**Files:**
- Modify: `/workspaces/ssmd/CLAUDE.md`

**Step 1: Update the connector section**

Add note about NATS-only:

```markdown
## Running the Connector

```bash
# Build first
make rust-build

# Run Kalshi connector (requires KALSHI_API_KEY, KALSHI_PRIVATE_KEY, and NATS)
./ssmd-rust/target/debug/ssmd-connector \
  --feed ./exchanges/feeds/kalshi.yaml \
  --env ./exchanges/environments/kalshi-local.yaml

# For demo API, also set KALSHI_USE_DEMO=true
```

The connector requires NATS transport. Configure in environment YAML:
```yaml
transport:
  transport_type: nats
  url: nats://localhost:4222
```
```

**Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for NATS-only connector"
```

---

## Phase 2: Archiver Service

### Task 7: Create ssmd-archiver Crate Structure

**Files:**
- Create: `ssmd-rust/crates/ssmd-archiver/Cargo.toml`
- Create: `ssmd-rust/crates/ssmd-archiver/src/main.rs`
- Create: `ssmd-rust/crates/ssmd-archiver/src/lib.rs`
- Modify: `ssmd-rust/Cargo.toml` (add to workspace)

**Step 1: Create the crate directory**

```bash
mkdir -p ssmd-rust/crates/ssmd-archiver/src
```

**Step 2: Create Cargo.toml**

Create `ssmd-rust/crates/ssmd-archiver/Cargo.toml`:

```toml
[package]
name = "ssmd-archiver"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "NATS to file archiver for SSMD market data"

[[bin]]
name = "ssmd-archiver"
path = "src/main.rs"

[dependencies]
tokio = { workspace = true }
async-trait = { workspace = true }
async-nats = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
serde_yaml = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
clap = { workspace = true }
flate2 = "1.0"

ssmd-middleware = { path = "../middleware" }
```

**Step 3: Create lib.rs stub**

Create `ssmd-rust/crates/ssmd-archiver/src/lib.rs`:

```rust
//! ssmd-archiver: NATS to file archiver for SSMD market data
//!
//! Subscribes to NATS JetStream and writes JSONL.gz files with
//! configurable rotation interval.

pub mod config;
pub mod error;
pub mod manifest;
pub mod subscriber;
pub mod writer;

pub use config::Config;
pub use error::ArchiverError;
```

**Step 4: Create main.rs stub**

Create `ssmd-rust/crates/ssmd-archiver/src/main.rs`:

```rust
//! ssmd-archiver binary entry point

use clap::Parser;
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "ssmd-archiver")]
#[command(about = "NATS to file archiver for SSMD market data")]
struct Args {
    /// Path to archiver configuration file
    #[arg(short, long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    info!(config = ?args.config, "Starting ssmd-archiver");

    // TODO: Load config, create subscriber, run archive loop
    info!("Archiver stub - implementation coming");

    Ok(())
}
```

**Step 5: Add to workspace**

Add to `ssmd-rust/Cargo.toml` members list:

```toml
members = [
    "crates/metadata",
    "crates/middleware",
    "crates/connector",
    "crates/ssmd-connector",
    "crates/schema",
    "crates/ssmd-archiver",
]
```

**Step 6: Verify it compiles**

Run: `cd ssmd-rust && cargo build -p ssmd-archiver`
Expected: Compile errors for missing modules

**Step 7: Create stub modules**

Create `ssmd-rust/crates/ssmd-archiver/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArchiverError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("NATS error: {0}")]
    Nats(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
```

Create `ssmd-rust/crates/ssmd-archiver/src/config.rs`:

```rust
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub nats: NatsConfig,
    pub storage: StorageConfig,
    pub rotation: RotationConfig,
}

#[derive(Debug, Deserialize)]
pub struct NatsConfig {
    pub url: String,
    pub stream: String,
    pub consumer: String,
    pub filter: String,
}

#[derive(Debug, Deserialize)]
pub struct StorageConfig {
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct RotationConfig {
    pub interval: String, // "15m", "1h", "1d"
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, crate::ArchiverError> {
        let content = std::fs::read_to_string(path)?;
        serde_yaml::from_str(&content)
            .map_err(|e| crate::ArchiverError::Config(e.to_string()))
    }
}
```

Create `ssmd-rust/crates/ssmd-archiver/src/manifest.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub feed: String,
    pub date: String,
    pub format: String,
    pub rotation_interval: String,
    pub files: Vec<FileEntry>,
    pub gaps: Vec<Gap>,
    pub tickers: Vec<String>,
    pub message_types: Vec<String>,
    pub has_gaps: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub records: u64,
    pub bytes: u64,
    pub nats_start_seq: u64,
    pub nats_end_seq: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Gap {
    pub after_seq: u64,
    pub missing_count: u64,
    pub detected_at: DateTime<Utc>,
}

impl Manifest {
    pub fn new(feed: &str, date: &str, rotation_interval: &str) -> Self {
        Self {
            feed: feed.to_string(),
            date: date.to_string(),
            format: "jsonl".to_string(),
            rotation_interval: rotation_interval.to_string(),
            files: Vec::new(),
            gaps: Vec::new(),
            tickers: Vec::new(),
            message_types: Vec::new(),
            has_gaps: false,
        }
    }
}
```

Create `ssmd-rust/crates/ssmd-archiver/src/subscriber.rs`:

```rust
// Placeholder for NATS subscriber
pub struct Subscriber;
```

Create `ssmd-rust/crates/ssmd-archiver/src/writer.rs`:

```rust
// Placeholder for JSONL.gz writer
pub struct ArchiveWriter;
```

**Step 8: Verify compilation**

Run: `cd ssmd-rust && cargo build -p ssmd-archiver`
Expected: Success

**Step 9: Commit**

```bash
git add ssmd-rust/Cargo.toml ssmd-rust/crates/ssmd-archiver/
git commit -m "feat(archiver): add ssmd-archiver crate structure"
```

---

### Task 8: Implement Config Loading

**Files:**
- Modify: `ssmd-rust/crates/ssmd-archiver/src/config.rs`
- Modify: `ssmd-rust/crates/ssmd-archiver/src/main.rs`
- Create: `ssmd-rust/crates/ssmd-archiver/archiver-example.yaml`

**Step 1: Write test for config loading**

Add to `config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config() {
        let yaml = r#"
nats:
  url: nats://localhost:4222
  stream: MARKETDATA
  consumer: archiver-kalshi
  filter: "prod.kalshi.json.>"

storage:
  path: /data/ssmd

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.nats.url, "nats://localhost:4222");
        assert_eq!(config.nats.stream, "MARKETDATA");
        assert_eq!(config.rotation.interval, "15m");
    }
}
```

**Step 2: Run test**

Run: `cd ssmd-rust && cargo test -p ssmd-archiver config`
Expected: PASS

**Step 3: Add parse_interval helper**

Add to `config.rs`:

```rust
use std::time::Duration;

impl RotationConfig {
    /// Parse interval string like "15m", "1h", "1d" to Duration
    pub fn parse_interval(&self) -> Result<Duration, crate::ArchiverError> {
        let s = self.interval.trim();
        if s.is_empty() {
            return Err(crate::ArchiverError::Config("Empty interval".to_string()));
        }

        let (num_str, unit) = s.split_at(s.len() - 1);
        let num: u64 = num_str.parse()
            .map_err(|_| crate::ArchiverError::Config(format!("Invalid interval: {}", s)))?;

        match unit {
            "s" => Ok(Duration::from_secs(num)),
            "m" => Ok(Duration::from_secs(num * 60)),
            "h" => Ok(Duration::from_secs(num * 60 * 60)),
            "d" => Ok(Duration::from_secs(num * 60 * 60 * 24)),
            _ => Err(crate::ArchiverError::Config(format!("Unknown unit: {}", unit))),
        }
    }
}
```

**Step 4: Add interval parsing test**

```rust
    #[test]
    fn test_parse_interval() {
        let config = RotationConfig { interval: "15m".to_string() };
        assert_eq!(config.parse_interval().unwrap(), Duration::from_secs(15 * 60));

        let config = RotationConfig { interval: "1h".to_string() };
        assert_eq!(config.parse_interval().unwrap(), Duration::from_secs(60 * 60));

        let config = RotationConfig { interval: "1d".to_string() };
        assert_eq!(config.parse_interval().unwrap(), Duration::from_secs(24 * 60 * 60));
    }
```

**Step 5: Run tests**

Run: `cd ssmd-rust && cargo test -p ssmd-archiver`
Expected: All PASS

**Step 6: Create example config**

Create `ssmd-rust/crates/ssmd-archiver/archiver-example.yaml`:

```yaml
# ssmd-archiver configuration example
nats:
  url: nats://localhost:4222
  stream: MARKETDATA
  consumer: archiver-kalshi
  filter: "prod.kalshi.json.>"

storage:
  path: /data/ssmd

rotation:
  interval: 15m   # 15m for testing, 1h or 1d for production
```

**Step 7: Update main.rs to load config**

```rust
//! ssmd-archiver binary entry point

use clap::Parser;
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_archiver::Config;

#[derive(Parser, Debug)]
#[command(name = "ssmd-archiver")]
#[command(about = "NATS to file archiver for SSMD market data")]
struct Args {
    /// Path to archiver configuration file
    #[arg(short, long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Load configuration
    let config = Config::load(&args.config).map_err(|e| {
        error!(error = %e, "Failed to load config");
        e
    })?;

    info!(
        nats_url = %config.nats.url,
        stream = %config.nats.stream,
        filter = %config.nats.filter,
        rotation = %config.rotation.interval,
        "Loaded configuration"
    );

    let rotation_duration = config.rotation.parse_interval()?;
    info!(rotation_secs = rotation_duration.as_secs(), "Rotation interval");

    // TODO: Create subscriber and run archive loop
    info!("Archiver stub - NATS subscriber coming next");

    Ok(())
}
```

**Step 8: Verify it runs**

Run: `cd ssmd-rust && cargo run -p ssmd-archiver -- --config crates/ssmd-archiver/archiver-example.yaml`
Expected: Logs showing config loaded

**Step 9: Commit**

```bash
git add ssmd-rust/crates/ssmd-archiver/
git commit -m "feat(archiver): implement config loading with interval parsing"
```

---

### Task 9: Implement JSONL.gz Writer

**Files:**
- Modify: `ssmd-rust/crates/ssmd-archiver/src/writer.rs`

**Step 1: Write test for writer**

```rust
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::error::ArchiverError;
use crate::manifest::FileEntry;

/// Writes JSONL.gz files with rotation
pub struct ArchiveWriter {
    base_path: PathBuf,
    feed: String,
    current_file: Option<CurrentFile>,
    rotation_minutes: u32,
}

struct CurrentFile {
    path: PathBuf,
    encoder: GzEncoder<File>,
    start_time: DateTime<Utc>,
    records: u64,
    bytes_written: u64,
    first_seq: Option<u64>,
    last_seq: Option<u64>,
}

impl ArchiveWriter {
    pub fn new(base_path: PathBuf, feed: String, rotation_minutes: u32) -> Self {
        Self {
            base_path,
            feed,
            current_file: None,
            rotation_minutes,
        }
    }

    /// Write a record to the current file, rotating if needed
    pub fn write(&mut self, data: &[u8], seq: u64, now: DateTime<Utc>) -> Result<(), ArchiverError> {
        // Check if we need to rotate
        if self.should_rotate(now) {
            self.rotate(now)?;
        }

        // Ensure we have a file open
        if self.current_file.is_none() {
            self.open_new_file(now)?;
        }

        let file = self.current_file.as_mut().unwrap();

        // Track sequence
        if file.first_seq.is_none() {
            file.first_seq = Some(seq);
        }
        file.last_seq = Some(seq);

        // Write the line
        file.encoder.write_all(data)?;
        file.encoder.write_all(b"\n")?;
        file.records += 1;
        file.bytes_written += data.len() as u64 + 1;

        Ok(())
    }

    /// Flush and close current file, returning FileEntry for manifest
    pub fn close(&mut self) -> Result<Option<FileEntry>, ArchiverError> {
        if let Some(file) = self.current_file.take() {
            let entry = self.finish_file(file)?;
            return Ok(Some(entry));
        }
        Ok(None)
    }

    fn should_rotate(&self, now: DateTime<Utc>) -> bool {
        if let Some(ref file) = self.current_file {
            let elapsed = now.signed_duration_since(file.start_time);
            elapsed.num_minutes() >= self.rotation_minutes as i64
        } else {
            false
        }
    }

    fn rotate(&mut self, now: DateTime<Utc>) -> Result<Option<FileEntry>, ArchiverError> {
        if let Some(file) = self.current_file.take() {
            let entry = self.finish_file(file)?;
            self.open_new_file(now)?;
            return Ok(Some(entry));
        }
        Ok(None)
    }

    fn open_new_file(&mut self, now: DateTime<Utc>) -> Result<(), ArchiverError> {
        let date_str = now.format("%Y-%m-%d").to_string();
        let time_str = now.format("%H%M").to_string();

        let dir = self.base_path.join(&self.feed).join(&date_str);
        fs::create_dir_all(&dir)?;

        let filename = format!("{}.jsonl.gz", time_str);
        let path = dir.join(&filename);

        let file = File::create(&path)?;
        let encoder = GzEncoder::new(file, Compression::default());

        self.current_file = Some(CurrentFile {
            path,
            encoder,
            start_time: now,
            records: 0,
            bytes_written: 0,
            first_seq: None,
            last_seq: None,
        });

        Ok(())
    }

    fn finish_file(&self, mut file: CurrentFile) -> Result<FileEntry, ArchiverError> {
        file.encoder.finish()?;

        let end_time = Utc::now();

        Ok(FileEntry {
            name: file.path.file_name().unwrap().to_string_lossy().to_string(),
            start: file.start_time,
            end: end_time,
            records: file.records,
            bytes: file.bytes_written,
            nats_start_seq: file.first_seq.unwrap_or(0),
            nats_end_seq: file.last_seq.unwrap_or(0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_write_records() {
        let tmp = TempDir::new().unwrap();
        let mut writer = ArchiveWriter::new(tmp.path().to_path_buf(), "kalshi".to_string(), 15);

        let now = Utc::now();
        writer.write(br#"{"type":"trade","ticker":"INXD"}"#, 1, now).unwrap();
        writer.write(br#"{"type":"trade","ticker":"KXBTC"}"#, 2, now).unwrap();

        let entry = writer.close().unwrap().unwrap();
        assert_eq!(entry.records, 2);
        assert_eq!(entry.nats_start_seq, 1);
        assert_eq!(entry.nats_end_seq, 2);

        // Verify file contents
        let date_str = now.format("%Y-%m-%d").to_string();
        let time_str = now.format("%H%M").to_string();
        let path = tmp.path().join("kalshi").join(&date_str).join(format!("{}.jsonl.gz", time_str));

        let file = File::open(&path).unwrap();
        let decoder = GzDecoder::new(file);
        let reader = BufReader::new(decoder);
        let lines: Vec<_> = reader.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].as_ref().unwrap().contains("INXD"));
        assert!(lines[1].as_ref().unwrap().contains("KXBTC"));
    }
}
```

**Step 2: Run test**

Run: `cd ssmd-rust && cargo test -p ssmd-archiver writer`
Expected: PASS

**Step 3: Commit**

```bash
git add ssmd-rust/crates/ssmd-archiver/src/writer.rs
git commit -m "feat(archiver): implement JSONL.gz writer with rotation"
```

---

### Task 10: Implement NATS JetStream Subscriber

**Files:**
- Modify: `ssmd-rust/crates/ssmd-archiver/src/subscriber.rs`

**Step 1: Implement subscriber**

```rust
use async_nats::jetstream::{self, consumer::PullConsumer, Context};
use tracing::{debug, error, info, warn};

use crate::config::NatsConfig;
use crate::error::ArchiverError;

pub struct Subscriber {
    consumer: PullConsumer,
    expected_seq: Option<u64>,
}

pub struct ReceivedMessage {
    pub data: Vec<u8>,
    pub seq: u64,
}

impl Subscriber {
    pub async fn connect(config: &NatsConfig) -> Result<Self, ArchiverError> {
        let client = async_nats::connect(&config.url)
            .await
            .map_err(|e| ArchiverError::Nats(e.to_string()))?;

        let jetstream = jetstream::new(client);

        // Get or create consumer
        let consumer = jetstream
            .get_stream(&config.stream)
            .await
            .map_err(|e| ArchiverError::Nats(format!("Stream not found: {}", e)))?
            .get_or_create_consumer(
                &config.consumer,
                jetstream::consumer::pull::Config {
                    durable_name: Some(config.consumer.clone()),
                    filter_subject: config.filter.clone(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ArchiverError::Nats(e.to_string()))?;

        info!(
            stream = %config.stream,
            consumer = %config.consumer,
            filter = %config.filter,
            "Connected to NATS JetStream"
        );

        Ok(Self {
            consumer,
            expected_seq: None,
        })
    }

    /// Fetch next batch of messages
    pub async fn fetch(&mut self, batch_size: usize) -> Result<Vec<ReceivedMessage>, ArchiverError> {
        let messages = self
            .consumer
            .fetch()
            .max_messages(batch_size)
            .messages()
            .await
            .map_err(|e| ArchiverError::Nats(e.to_string()))?;

        use futures_util::StreamExt;
        let mut result = Vec::new();

        tokio::pin!(messages);
        while let Some(msg_result) = messages.next().await {
            match msg_result {
                Ok(msg) => {
                    let seq = msg.info().map(|i| i.stream_sequence).unwrap_or(0);

                    // Check for gaps
                    if let Some(expected) = self.expected_seq {
                        if seq > expected {
                            warn!(
                                expected = expected,
                                actual = seq,
                                gap = seq - expected,
                                "Gap detected in sequence"
                            );
                        }
                    }
                    self.expected_seq = Some(seq + 1);

                    result.push(ReceivedMessage {
                        data: msg.payload.to_vec(),
                        seq,
                    });

                    // Ack the message
                    if let Err(e) = msg.ack().await {
                        error!(error = %e, seq = seq, "Failed to ack message");
                    }
                }
                Err(e) => {
                    error!(error = %e, "Error receiving message");
                }
            }
        }

        debug!(count = result.len(), "Fetched messages");
        Ok(result)
    }

    /// Check if there was a gap (returns gap info for manifest)
    pub fn check_gap(&self, seq: u64) -> Option<(u64, u64)> {
        if let Some(expected) = self.expected_seq {
            if seq > expected {
                return Some((expected - 1, seq - expected));
            }
        }
        None
    }
}
```

**Step 2: Add futures-util dependency**

Add to `Cargo.toml`:
```toml
futures-util = { workspace = true }
```

**Step 3: Verify compilation**

Run: `cd ssmd-rust && cargo build -p ssmd-archiver`
Expected: Success

**Step 4: Commit**

```bash
git add ssmd-rust/crates/ssmd-archiver/
git commit -m "feat(archiver): implement NATS JetStream subscriber with gap detection"
```

---

### Task 11: Implement Main Archive Loop

**Files:**
- Modify: `ssmd-rust/crates/ssmd-archiver/src/main.rs`

**Step 1: Implement the full main loop**

```rust
//! ssmd-archiver binary entry point

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;
use tokio::signal;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_archiver::{Config, manifest::{Gap, Manifest}, subscriber::Subscriber, writer::ArchiveWriter};

#[derive(Parser, Debug)]
#[command(name = "ssmd-archiver")]
#[command(about = "NATS to file archiver for SSMD market data")]
struct Args {
    /// Path to archiver configuration file
    #[arg(short, long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Load configuration
    let config = Config::load(&args.config).map_err(|e| {
        error!(error = %e, "Failed to load config");
        e
    })?;

    info!(
        nats_url = %config.nats.url,
        stream = %config.nats.stream,
        filter = %config.nats.filter,
        rotation = %config.rotation.interval,
        storage = ?config.storage.path,
        "Starting archiver"
    );

    let rotation_duration = config.rotation.parse_interval()?;
    let rotation_minutes = (rotation_duration.as_secs() / 60) as u32;

    // Extract feed name from filter (e.g., "prod.kalshi.json.>" -> "kalshi")
    let feed = extract_feed_from_filter(&config.nats.filter)
        .ok_or("Could not extract feed from filter")?;

    // Connect to NATS
    let mut subscriber = Subscriber::connect(&config.nats).await?;

    // Create writer
    let mut writer = ArchiveWriter::new(
        config.storage.path.clone(),
        feed.clone(),
        rotation_minutes,
    );

    // Track manifest data
    let mut tickers: HashSet<String> = HashSet::new();
    let mut message_types: HashSet<String> = HashSet::new();
    let mut gaps: Vec<Gap> = Vec::new();
    let mut current_date = Utc::now().format("%Y-%m-%d").to_string();

    // Fetch interval
    let mut fetch_interval = interval(Duration::from_millis(100));

    info!("Archiver running, press Ctrl+C to stop");

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Shutdown signal received");
                break;
            }
            _ = fetch_interval.tick() => {
                let now = Utc::now();
                let date = now.format("%Y-%m-%d").to_string();

                // Check for day rollover
                if date != current_date {
                    info!(old = %current_date, new = %date, "Day rollover, writing manifest");
                    write_manifest(&config.storage.path, &feed, &current_date, &config.rotation.interval, &mut writer, &tickers, &message_types, &gaps)?;
                    tickers.clear();
                    message_types.clear();
                    gaps.clear();
                    current_date = date;
                }

                // Fetch messages
                match subscriber.fetch(100).await {
                    Ok(messages) => {
                        for msg in messages {
                            // Check for gap
                            if let Some((after_seq, missing)) = subscriber.check_gap(msg.seq) {
                                warn!(after_seq = after_seq, missing = missing, "Recording gap");
                                gaps.push(Gap {
                                    after_seq,
                                    missing_count: missing,
                                    detected_at: now,
                                });
                            }

                            // Extract ticker and type for manifest
                            if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&msg.data) {
                                if let Some(msg_type) = parsed.get("type").and_then(|v| v.as_str()) {
                                    message_types.insert(msg_type.to_string());
                                }
                                if let Some(inner) = parsed.get("msg") {
                                    if let Some(ticker) = inner.get("market_ticker").and_then(|v| v.as_str()) {
                                        tickers.insert(ticker.to_string());
                                    }
                                }
                            }

                            // Write to file
                            if let Err(e) = writer.write(&msg.data, msg.seq, now) {
                                error!(error = %e, "Failed to write message");
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to fetch messages");
                    }
                }
            }
        }
    }

    // Final cleanup
    info!("Writing final manifest");
    write_manifest(&config.storage.path, &feed, &current_date, &config.rotation.interval, &mut writer, &tickers, &message_types, &gaps)?;

    info!("Archiver stopped");
    Ok(())
}

fn extract_feed_from_filter(filter: &str) -> Option<String> {
    // Filter format: "{env}.{feed}.json.>" or "{env}.{feed}.json.{type}.>"
    let parts: Vec<&str> = filter.split('.').collect();
    if parts.len() >= 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

fn write_manifest(
    base_path: &PathBuf,
    feed: &str,
    date: &str,
    rotation_interval: &str,
    writer: &mut ArchiveWriter,
    tickers: &HashSet<String>,
    message_types: &HashSet<String>,
    gaps: &[Gap],
) -> Result<(), Box<dyn std::error::Error>> {
    // Close current file and get entry
    let file_entry = writer.close()?;

    let mut manifest = Manifest::new(feed, date, rotation_interval);
    if let Some(entry) = file_entry {
        manifest.files.push(entry);
    }
    manifest.tickers = tickers.iter().cloned().collect();
    manifest.message_types = message_types.iter().cloned().collect();
    manifest.gaps = gaps.to_vec();
    manifest.has_gaps = !gaps.is_empty();

    // Write manifest
    let manifest_path = base_path.join(feed).join(date).join("manifest.json");
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, manifest_json)?;

    info!(path = ?manifest_path, "Wrote manifest");
    Ok(())
}
```

**Step 2: Verify compilation**

Run: `cd ssmd-rust && cargo build -p ssmd-archiver`
Expected: Success

**Step 3: Commit**

```bash
git add ssmd-rust/crates/ssmd-archiver/src/main.rs
git commit -m "feat(archiver): implement main archive loop with manifest generation"
```

---

### Task 12: Add Dockerfile and CI

**Files:**
- Create: `ssmd-rust/crates/ssmd-archiver/Dockerfile`
- Create: `.github/workflows/build-archiver.yaml`

**Step 1: Create Dockerfile**

Create `ssmd-rust/crates/ssmd-archiver/Dockerfile`:

```dockerfile
FROM rust:1.83-slim as builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    capnproto \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace
COPY . .

# Build release binary
RUN cargo build --release --package ssmd-archiver

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/ssmd-archiver /usr/local/bin/

ENTRYPOINT ["ssmd-archiver"]
```

**Step 2: Create CI workflow**

Create `.github/workflows/build-archiver.yaml`:

```yaml
name: Build ssmd-archiver

on:
  push:
    tags:
      - 'v*'

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository_owner }}/ssmd-archiver

jobs:
  build:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write

    steps:
      - uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Login to Container Registry
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Extract metadata
        id: meta
        uses: docker/metadata-action@v5
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}
          tags: |
            type=semver,pattern={{version}}
            type=semver,pattern={{major}}.{{minor}}

      - name: Build and push
        uses: docker/build-push-action@v6
        with:
          context: ./ssmd-rust
          file: ./ssmd-rust/crates/ssmd-archiver/Dockerfile
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

**Step 3: Commit**

```bash
git add ssmd-rust/crates/ssmd-archiver/Dockerfile .github/workflows/build-archiver.yaml
git commit -m "feat(archiver): add Dockerfile and CI workflow"
```

---

### Task 13: Run Full Test Suite and Final Commit

**Step 1: Run all tests**

Run: `cd ssmd-rust && cargo test`
Expected: All tests PASS

**Step 2: Run clippy**

Run: `cd ssmd-rust && cargo clippy --all-targets`
Expected: No errors (warnings OK for now)

**Step 3: Build all packages**

Run: `cd ssmd-rust && cargo build --release`
Expected: Success

**Step 4: Push branch**

```bash
git push origin feature/connector-nats-only-archiver
```

---

## Summary

| Task | Component | Description |
|------|-----------|-------------|
| 1 | SubjectBuilder | Add json_* methods |
| 2 | NatsWriter | Simplify to raw JSON passthrough |
| 3 | lib.rs | Hide FileWriter export |
| 4 | main.rs | Require NATS transport |
| 5 | Tests | Verify integration tests |
| 6 | CLAUDE.md | Update docs |
| 7 | ssmd-archiver | Create crate structure |
| 8 | Config | Implement config loading |
| 9 | Writer | Implement JSONL.gz writer |
| 10 | Subscriber | Implement NATS subscriber |
| 11 | Main | Implement archive loop |
| 12 | CI | Add Dockerfile and workflow |
| 13 | Final | Run tests and push |
