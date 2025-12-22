# Runtime Framework Implementation Plan (Rust)

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create `ssmd-connector` Rust binary that reads metadata configs, connects to Kalshi WebSocket, and writes raw messages to JSONL files.

**Architecture:** Cargo workspace with trait-based framework. Pluggable Connector, Writer, and KeyResolver. Runner wires components together. Axum HTTP server for health/metrics.

**Tech Stack:** Rust 2021, tokio, tokio-tungstenite, axum, serde/serde_yaml, clap, tracing

---

## Phase 1: Cargo Workspace Setup

### Task 1: Initialize Cargo workspace

**Files:**
- Create: `ssmd-connector/Cargo.toml` (workspace root)
- Create: `ssmd-connector/crates/connector/Cargo.toml`
- Create: `ssmd-connector/crates/connector/src/lib.rs`

**Step 1: Create workspace directory structure**

```bash
mkdir -p ssmd-connector/crates/connector/src
```

**Step 2: Create workspace Cargo.toml**

Create `ssmd-connector/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/connector",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
authors = ["ssmd contributors"]

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
thiserror = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

**Step 3: Create connector crate Cargo.toml**

Create `ssmd-connector/crates/connector/Cargo.toml`:

```toml
[package]
name = "ssmd-connector"
version.workspace = true
edition.workspace = true

[dependencies]
tokio = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
```

**Step 4: Create minimal lib.rs**

Create `ssmd-connector/crates/connector/src/lib.rs`:

```rust
//! ssmd-connector: Core runtime library for market data collection

pub mod traits;
pub mod message;
```

**Step 5: Verify workspace compiles**

Run: `cd ssmd-connector && cargo build`
Expected: Success (with warnings about empty modules)

**Step 6: Commit**

```bash
git add ssmd-connector/
git commit -m "feat(connector): initialize Rust workspace"
```

---

### Task 2: Create core traits

**Files:**
- Create: `ssmd-connector/crates/connector/src/traits.rs`
- Create: `ssmd-connector/crates/connector/src/message.rs`
- Create: `ssmd-connector/crates/connector/src/error.rs`
- Modify: `ssmd-connector/crates/connector/src/lib.rs`

**Step 1: Create error types**

Create `ssmd-connector/crates/connector/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectorError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    #[error("disconnected: {0}")]
    Disconnected(String),
}

#[derive(Error, Debug)]
pub enum WriterError {
    #[error("write failed: {0}")]
    WriteFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum ResolverError {
    #[error("unsupported source: {0}")]
    UnsupportedSource(String),
    #[error("missing key: {0}")]
    MissingKey(String),
}
```

**Step 2: Create message struct**

Create `ssmd-connector/crates/connector/src/message.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Message wraps raw data with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// ISO 8601 timestamp
    pub ts: String,
    /// Feed name
    pub feed: String,
    /// Raw message data (stored as raw JSON value)
    pub data: serde_json::Value,
}

impl Message {
    pub fn new(feed: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            ts: chrono::Utc::now().to_rfc3339(),
            feed: feed.into(),
            data,
        }
    }
}
```

**Step 3: Add chrono dependency**

Add to `ssmd-connector/Cargo.toml` workspace dependencies:

```toml
chrono = { version = "0.4", features = ["serde"] }
```

Add to `ssmd-connector/crates/connector/Cargo.toml`:

```toml
chrono = { workspace = true }
```

**Step 4: Create traits**

Create `ssmd-connector/crates/connector/src/traits.rs`:

```rust
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::error::{ConnectorError, ResolverError, WriterError};
use crate::message::Message;

/// Connector trait for data sources (WebSocket, REST, etc.)
#[async_trait]
pub trait Connector: Send + Sync {
    /// Establish connection to the data source
    async fn connect(&mut self) -> Result<(), ConnectorError>;

    /// Get receiver for incoming messages
    fn messages(&mut self) -> mpsc::Receiver<Vec<u8>>;

    /// Close the connection
    async fn close(&mut self) -> Result<(), ConnectorError>;
}

/// Writer trait for output destinations (file, S3, NATS, etc.)
#[async_trait]
pub trait Writer: Send + Sync {
    /// Write a message to the destination
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError>;

    /// Close and flush the writer
    async fn close(&mut self) -> Result<(), WriterError>;
}

/// KeyResolver trait for credential sources (env vars, Vault, etc.)
pub trait KeyResolver: Send + Sync {
    /// Resolve keys from a source string (e.g., "env:VAR1,VAR2")
    fn resolve(&self, source: &str) -> Result<HashMap<String, String>, ResolverError>;
}
```

**Step 5: Update lib.rs**

Update `ssmd-connector/crates/connector/src/lib.rs`:

```rust
//! ssmd-connector: Core runtime library for market data collection

pub mod error;
pub mod message;
pub mod traits;

pub use error::{ConnectorError, ResolverError, WriterError};
pub use message::Message;
pub use traits::{Connector, KeyResolver, Writer};
```

**Step 6: Verify it compiles**

Run: `cd ssmd-connector && cargo build`
Expected: Success

**Step 7: Commit**

```bash
git add ssmd-connector/
git commit -m "feat(connector): add core traits and error types"
```

---

## Phase 2: Key Resolver

### Task 3: Create EnvResolver

**Files:**
- Create: `ssmd-connector/crates/connector/src/resolver/mod.rs`
- Create: `ssmd-connector/crates/connector/src/resolver/env.rs`
- Modify: `ssmd-connector/crates/connector/src/lib.rs`

**Step 1: Write test for EnvResolver**

Create `ssmd-connector/crates/connector/src/resolver/env.rs`:

```rust
use std::collections::HashMap;
use std::env;

use crate::error::ResolverError;
use crate::traits::KeyResolver;

/// Resolves keys from environment variables
pub struct EnvResolver;

impl EnvResolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnvResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyResolver for EnvResolver {
    /// Parses "env:VAR1,VAR2" and returns values from environment
    fn resolve(&self, source: &str) -> Result<HashMap<String, String>, ResolverError> {
        let prefix = "env:";
        if !source.starts_with(prefix) {
            return Err(ResolverError::UnsupportedSource(format!(
                "expected 'env:' prefix, got: {}",
                source
            )));
        }

        let vars_part = &source[prefix.len()..];
        if vars_part.is_empty() {
            return Err(ResolverError::UnsupportedSource(
                "empty env source".to_string(),
            ));
        }

        let mut result = HashMap::new();
        for var in vars_part.split(',') {
            let var = var.trim();
            if var.is_empty() {
                continue;
            }
            match env::var(var) {
                Ok(value) => {
                    result.insert(var.to_string(), value);
                }
                Err(_) => {
                    return Err(ResolverError::MissingKey(var.to_string()));
                }
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_env_vars() {
        env::set_var("TEST_KEY1", "value1");
        env::set_var("TEST_KEY2", "value2");

        let resolver = EnvResolver::new();
        let result = resolver.resolve("env:TEST_KEY1,TEST_KEY2").unwrap();

        assert_eq!(result.get("TEST_KEY1"), Some(&"value1".to_string()));
        assert_eq!(result.get("TEST_KEY2"), Some(&"value2".to_string()));

        env::remove_var("TEST_KEY1");
        env::remove_var("TEST_KEY2");
    }

    #[test]
    fn test_missing_var() {
        let resolver = EnvResolver::new();
        let result = resolver.resolve("env:NONEXISTENT_VAR_12345");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_source() {
        let resolver = EnvResolver::new();
        let result = resolver.resolve("vault:secret/path");
        assert!(result.is_err());
    }
}
```

**Step 2: Create resolver module**

Create `ssmd-connector/crates/connector/src/resolver/mod.rs`:

```rust
mod env;

pub use env::EnvResolver;
```

**Step 3: Update lib.rs**

Add to `ssmd-connector/crates/connector/src/lib.rs`:

```rust
pub mod resolver;

pub use resolver::EnvResolver;
```

**Step 4: Run tests**

Run: `cd ssmd-connector && cargo test`
Expected: All tests pass

**Step 5: Commit**

```bash
git add ssmd-connector/
git commit -m "feat(connector): add EnvResolver for environment variables"
```

---

## Phase 3: File Writer

### Task 4: Create FileWriter

**Files:**
- Create: `ssmd-connector/crates/connector/src/writer/mod.rs`
- Create: `ssmd-connector/crates/connector/src/writer/file.rs`
- Modify: `ssmd-connector/crates/connector/src/lib.rs`

**Step 1: Create FileWriter implementation**

Create `ssmd-connector/crates/connector/src/writer/file.rs`:

```rust
use async_trait::async_trait;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::WriterError;
use crate::message::Message;
use crate::traits::Writer;

/// Writes messages to date-partitioned JSONL files
pub struct FileWriter {
    base_dir: PathBuf,
    feed_name: String,
    inner: Mutex<FileWriterInner>,
}

struct FileWriterInner {
    writer: Option<BufWriter<File>>,
    current_date: String,
}

impl FileWriter {
    pub fn new(base_dir: impl Into<PathBuf>, feed_name: impl Into<String>) -> Self {
        Self {
            base_dir: base_dir.into(),
            feed_name: feed_name.into(),
            inner: Mutex::new(FileWriterInner {
                writer: None,
                current_date: String::new(),
            }),
        }
    }

    fn get_date_from_ts(ts: &str) -> String {
        // Extract YYYY-MM-DD from ISO 8601 timestamp
        ts.get(..10).unwrap_or("unknown").to_string()
    }
}

#[async_trait]
impl Writer for FileWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        let date = Self::get_date_from_ts(&msg.ts);

        let mut inner = self.inner.lock().unwrap();

        // Rotate file if date changed
        if date != inner.current_date {
            if let Some(ref mut writer) = inner.writer {
                writer.flush()?;
            }

            let dir = self.base_dir.join(&date);
            fs::create_dir_all(&dir)?;

            let path = dir.join(format!("{}.jsonl", self.feed_name));
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;

            inner.writer = Some(BufWriter::new(file));
            inner.current_date = date;
        }

        // Write JSON line
        if let Some(ref mut writer) = inner.writer {
            let line = serde_json::to_string(msg)
                .map_err(|e| WriterError::WriteFailed(e.to_string()))?;
            writeln!(writer, "{}", line)?;
        }

        Ok(())
    }

    async fn close(&mut self) -> Result<(), WriterError> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(ref mut writer) = inner.writer {
            writer.flush()?;
        }
        inner.writer = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_write_message() {
        let tmp_dir = TempDir::new().unwrap();
        let mut writer = FileWriter::new(tmp_dir.path(), "test-feed");

        let msg = Message {
            ts: "2025-12-22T10:30:00Z".to_string(),
            feed: "test-feed".to_string(),
            data: serde_json::json!({"price": 100}),
        };

        writer.write(&msg).await.unwrap();
        writer.close().await.unwrap();

        let expected_path = tmp_dir.path().join("2025-12-22").join("test-feed.jsonl");
        assert!(expected_path.exists());

        let content = fs::read_to_string(expected_path).unwrap();
        assert!(content.contains("\"price\":100"));
    }

    #[tokio::test]
    async fn test_date_partitioning() {
        let tmp_dir = TempDir::new().unwrap();
        let mut writer = FileWriter::new(tmp_dir.path(), "test-feed");

        let msg1 = Message {
            ts: "2025-12-22T10:30:00Z".to_string(),
            feed: "test-feed".to_string(),
            data: serde_json::json!({"day": 22}),
        };
        let msg2 = Message {
            ts: "2025-12-23T10:30:00Z".to_string(),
            feed: "test-feed".to_string(),
            data: serde_json::json!({"day": 23}),
        };

        writer.write(&msg1).await.unwrap();
        writer.write(&msg2).await.unwrap();
        writer.close().await.unwrap();

        assert!(tmp_dir.path().join("2025-12-22").join("test-feed.jsonl").exists());
        assert!(tmp_dir.path().join("2025-12-23").join("test-feed.jsonl").exists());
    }
}
```

**Step 2: Add tempfile dev dependency**

Add to `ssmd-connector/Cargo.toml` workspace dependencies:

```toml
tempfile = "3"
```

Add to `ssmd-connector/crates/connector/Cargo.toml`:

```toml
[dev-dependencies]
tempfile = { workspace = true }
```

**Step 3: Create writer module**

Create `ssmd-connector/crates/connector/src/writer/mod.rs`:

```rust
mod file;

pub use file::FileWriter;
```

**Step 4: Update lib.rs**

Add to `ssmd-connector/crates/connector/src/lib.rs`:

```rust
pub mod writer;

pub use writer::FileWriter;
```

**Step 5: Run tests**

Run: `cd ssmd-connector && cargo test`
Expected: All tests pass

**Step 6: Commit**

```bash
git add ssmd-connector/
git commit -m "feat(connector): add FileWriter for JSONL output"
```

---

## Phase 4: WebSocket Connector

### Task 5: Create WebSocket connector

**Files:**
- Create: `ssmd-connector/crates/connector/src/websocket/mod.rs`
- Create: `ssmd-connector/crates/connector/src/websocket/kalshi.rs`
- Modify: `ssmd-connector/crates/connector/src/lib.rs`
- Modify: `ssmd-connector/Cargo.toml` (add tokio-tungstenite)

**Step 1: Add WebSocket dependencies**

Add to `ssmd-connector/Cargo.toml` workspace dependencies:

```toml
tokio-tungstenite = { version = "0.21", features = ["native-tls"] }
futures-util = "0.3"
url = "2"
```

Add to `ssmd-connector/crates/connector/Cargo.toml`:

```toml
tokio-tungstenite = { workspace = true }
futures-util = { workspace = true }
url = { workspace = true }
```

**Step 2: Create WebSocket connector**

Create `ssmd-connector/crates/connector/src/websocket/kalshi.rs`:

```rust
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use url::Url;

use crate::error::ConnectorError;
use crate::traits::Connector;

/// WebSocket connector for Kalshi
pub struct WebSocketConnector {
    url: String,
    creds: Option<HashMap<String, String>>,
    tx: Option<mpsc::Sender<Vec<u8>>>,
    rx: Option<mpsc::Receiver<Vec<u8>>>,
}

impl WebSocketConnector {
    pub fn new(url: impl Into<String>, creds: Option<HashMap<String, String>>) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            url: url.into(),
            creds,
            tx: Some(tx),
            rx: Some(rx),
        }
    }
}

#[async_trait]
impl Connector for WebSocketConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        let url = Url::parse(&self.url)
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        let (mut write, mut read) = ws_stream.split();
        let tx = self.tx.take().unwrap();

        // Handle authentication if credentials provided
        if let Some(ref creds) = self.creds {
            if let (Some(api_key), Some(api_secret)) =
                (creds.get("KALSHI_API_KEY"), creds.get("KALSHI_API_SECRET"))
            {
                // Send auth message (Kalshi-specific format)
                let auth_msg = serde_json::json!({
                    "type": "auth",
                    "api_key": api_key,
                    "api_secret": api_secret
                });
                write
                    .send(WsMessage::Text(auth_msg.to_string()))
                    .await
                    .map_err(|e| ConnectorError::AuthFailed(e.to_string()))?;
            }
        }

        // Spawn reader task
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(WsMessage::Text(text)) => {
                        if tx.send(text.into_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Ok(WsMessage::Binary(data)) => {
                        if tx.send(data).await.is_err() {
                            break;
                        }
                    }
                    Ok(WsMessage::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        Ok(())
    }

    fn messages(&mut self) -> mpsc::Receiver<Vec<u8>> {
        self.rx.take().expect("messages() called twice")
    }

    async fn close(&mut self) -> Result<(), ConnectorError> {
        // Drop sender to signal reader task to stop
        self.tx = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_connector() {
        let creds = HashMap::from([
            ("KALSHI_API_KEY".to_string(), "test-key".to_string()),
            ("KALSHI_API_SECRET".to_string(), "test-secret".to_string()),
        ]);

        let connector = WebSocketConnector::new("wss://example.com/ws", Some(creds));
        assert_eq!(connector.url, "wss://example.com/ws");
    }

    #[test]
    fn test_messages_channel() {
        let mut connector = WebSocketConnector::new("wss://example.com/ws", None);
        let _rx = connector.messages();
        // Channel should be returned successfully
    }
}
```

**Step 3: Create websocket module**

Create `ssmd-connector/crates/connector/src/websocket/mod.rs`:

```rust
mod kalshi;

pub use kalshi::WebSocketConnector;
```

**Step 4: Update lib.rs**

Add to `ssmd-connector/crates/connector/src/lib.rs`:

```rust
pub mod websocket;

pub use websocket::WebSocketConnector;
```

**Step 5: Run tests**

Run: `cd ssmd-connector && cargo test`
Expected: All tests pass

**Step 6: Commit**

```bash
git add ssmd-connector/
git commit -m "feat(connector): add WebSocket connector for Kalshi"
```

---

## Phase 5: Runner

### Task 6: Create Runner

**Files:**
- Create: `ssmd-connector/crates/connector/src/runner.rs`
- Modify: `ssmd-connector/crates/connector/src/lib.rs`

**Step 1: Create Runner**

Create `ssmd-connector/crates/connector/src/runner.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::select;
use tracing::{error, info};

use crate::error::ConnectorError;
use crate::message::Message;
use crate::traits::{Connector, Writer};

/// Runner orchestrates the data collection pipeline
pub struct Runner<C: Connector, W: Writer> {
    feed_name: String,
    connector: C,
    writer: W,
    connected: Arc<AtomicBool>,
}

impl<C: Connector, W: Writer> Runner<C, W> {
    pub fn new(feed_name: impl Into<String>, connector: C, writer: W) -> Self {
        Self {
            feed_name: feed_name.into(),
            connector,
            writer,
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns whether the connector is currently connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Returns a handle to the connected status
    pub fn connected_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.connected)
    }

    /// Run the collection pipeline until cancelled or disconnected
    pub async fn run(&mut self, shutdown: tokio::sync::watch::Receiver<bool>) -> Result<(), ConnectorError> {
        // Connect
        self.connector.connect().await?;
        self.connected.store(true, Ordering::SeqCst);
        info!(feed = %self.feed_name, "Connected to data source");

        let mut rx = self.connector.messages();
        let mut shutdown = shutdown;

        loop {
            select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Shutdown signal received");
                        break;
                    }
                }
                msg = rx.recv() => {
                    match msg {
                        Some(data) => {
                            // Parse as JSON and wrap with metadata
                            let json_data = match serde_json::from_slice(&data) {
                                Ok(v) => v,
                                Err(_) => {
                                    // If not valid JSON, store as string
                                    serde_json::Value::String(
                                        String::from_utf8_lossy(&data).to_string()
                                    )
                                }
                            };

                            let message = Message::new(&self.feed_name, json_data);

                            if let Err(e) = self.writer.write(&message).await {
                                error!(error = %e, "Failed to write message");
                                // Continue on write errors
                            }
                        }
                        None => {
                            // Channel closed - connector disconnected
                            self.connected.store(false, Ordering::SeqCst);
                            info!("Connector disconnected");
                            break;
                        }
                    }
                }
            }
        }

        // Cleanup
        self.connected.store(false, Ordering::SeqCst);
        self.writer.close().await.ok();
        self.connector.close().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::mpsc;

    struct MockConnector {
        tx: mpsc::Sender<Vec<u8>>,
        rx: Option<mpsc::Receiver<Vec<u8>>>,
    }

    impl MockConnector {
        fn new() -> (Self, mpsc::Sender<Vec<u8>>) {
            let (tx, rx) = mpsc::channel(10);
            let tx_clone = tx.clone();
            (
                Self {
                    tx,
                    rx: Some(rx),
                },
                tx_clone,
            )
        }
    }

    #[async_trait]
    impl Connector for MockConnector {
        async fn connect(&mut self) -> Result<(), ConnectorError> {
            Ok(())
        }
        fn messages(&mut self) -> mpsc::Receiver<Vec<u8>> {
            self.rx.take().unwrap()
        }
        async fn close(&mut self) -> Result<(), ConnectorError> {
            Ok(())
        }
    }

    struct MockWriter {
        write_count: Arc<AtomicUsize>,
    }

    impl MockWriter {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let count = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    write_count: Arc::clone(&count),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl Writer for MockWriter {
        async fn write(&mut self, _msg: &Message) -> Result<(), crate::error::WriterError> {
            self.write_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn close(&mut self) -> Result<(), crate::error::WriterError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_runner_processes_messages() {
        let (connector, msg_tx) = MockConnector::new();
        let (writer, write_count) = MockWriter::new();

        let mut runner = Runner::new("test-feed", connector, writer);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Spawn runner
        let handle = tokio::spawn(async move {
            runner.run(shutdown_rx).await
        });

        // Send a message
        msg_tx.send(b"{\"test\":true}".to_vec()).await.unwrap();

        // Wait a bit for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Shutdown
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap().unwrap();

        assert!(write_count.load(Ordering::SeqCst) >= 1);
    }
}
```

**Step 2: Update lib.rs**

Add to `ssmd-connector/crates/connector/src/lib.rs`:

```rust
pub mod runner;

pub use runner::Runner;
```

**Step 3: Run tests**

Run: `cd ssmd-connector && cargo test`
Expected: All tests pass

**Step 4: Commit**

```bash
git add ssmd-connector/
git commit -m "feat(connector): add Runner for pipeline orchestration"
```

---

## Phase 6: Health Server

### Task 7: Create health server

**Files:**
- Create: `ssmd-connector/crates/connector/src/server/mod.rs`
- Create: `ssmd-connector/crates/connector/src/server/health.rs`
- Modify: `ssmd-connector/Cargo.toml` (add axum)
- Modify: `ssmd-connector/crates/connector/src/lib.rs`

**Step 1: Add axum dependency**

Add to `ssmd-connector/Cargo.toml` workspace dependencies:

```toml
axum = "0.7"
```

Add to `ssmd-connector/crates/connector/Cargo.toml`:

```toml
axum = { workspace = true }
```

**Step 2: Create health server**

Create `ssmd-connector/crates/connector/src/server/health.rs`:

```rust
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Clone)]
pub struct HealthState {
    pub connected: Arc<AtomicBool>,
    pub messages_total: Arc<AtomicU64>,
    pub errors_total: Arc<AtomicU64>,
}

impl HealthState {
    pub fn new(connected: Arc<AtomicBool>) -> Self {
        Self {
            connected,
            messages_total: Arc::new(AtomicU64::new(0)),
            errors_total: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record_message(&self) {
        self.messages_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.errors_total.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ReadyResponse {
    status: &'static str,
}

async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse { status: "ok" })
}

async fn ready_handler(State(state): State<HealthState>) -> impl IntoResponse {
    if state.connected.load(Ordering::SeqCst) {
        (StatusCode::OK, Json(ReadyResponse { status: "ready" }))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse { status: "not_ready" }),
        )
    }
}

async fn metrics_handler(State(state): State<HealthState>) -> impl IntoResponse {
    let connected = if state.connected.load(Ordering::SeqCst) { 1 } else { 0 };
    let messages = state.messages_total.load(Ordering::Relaxed);
    let errors = state.errors_total.load(Ordering::Relaxed);

    let body = format!(
        "# HELP ssmd_messages_total Total messages received\n\
         # TYPE ssmd_messages_total counter\n\
         ssmd_messages_total {}\n\
         # HELP ssmd_errors_total Total errors\n\
         # TYPE ssmd_errors_total counter\n\
         ssmd_errors_total {}\n\
         # HELP ssmd_connected Connection status\n\
         # TYPE ssmd_connected gauge\n\
         ssmd_connected {}\n",
        messages, errors, connected
    );

    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        body,
    )
}

pub fn create_router(state: HealthState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .route("/metrics", get(metrics_handler))
        .with_state(state)
}

pub async fn serve(addr: SocketAddr, state: HealthState) -> std::io::Result<()> {
    let app = create_router(state);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_ok() {
        let state = HealthState::new(Arc::new(AtomicBool::new(true)));
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_when_connected() {
        let state = HealthState::new(Arc::new(AtomicBool::new(true)));
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_when_disconnected() {
        let state = HealthState::new(Arc::new(AtomicBool::new(false)));
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
```

**Step 3: Add tower dev dependency for tests**

Add to `ssmd-connector/Cargo.toml` workspace dependencies:

```toml
tower = { version = "0.4", features = ["util"] }
```

Add to `ssmd-connector/crates/connector/Cargo.toml`:

```toml
[dev-dependencies]
tower = { workspace = true }
```

**Step 4: Create server module**

Create `ssmd-connector/crates/connector/src/server/mod.rs`:

```rust
mod health;

pub use health::{create_router, serve, HealthState};
```

**Step 5: Update lib.rs**

Add to `ssmd-connector/crates/connector/src/lib.rs`:

```rust
pub mod server;

pub use server::{serve as serve_health, HealthState};
```

**Step 6: Run tests**

Run: `cd ssmd-connector && cargo test`
Expected: All tests pass

**Step 7: Commit**

```bash
git add ssmd-connector/
git commit -m "feat(connector): add health/ready/metrics server"
```

---

## Phase 7: Config Loading

### Task 8: Create config loader

**Files:**
- Create: `ssmd-connector/crates/connector/src/config.rs`
- Modify: `ssmd-connector/crates/connector/src/lib.rs`

**Step 1: Create config loader**

Create `ssmd-connector/crates/connector/src/config.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub feed: String,
    pub keys: Option<Vec<KeySpec>>,
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySpec {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(rename = "type")]
    pub storage_type: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feed {
    pub name: String,
    #[serde(rename = "type")]
    pub feed_type: String,
    pub versions: Vec<FeedVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedVersion {
    pub version: String,
    pub endpoint: String,
    pub active: Option<bool>,
}

impl Feed {
    pub fn active_version(&self) -> Option<&FeedVersion> {
        self.versions.iter().find(|v| v.active.unwrap_or(false))
    }
}

pub fn load_environment(path: &Path) -> Result<Environment, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let env: Environment = serde_yaml::from_str(&content)?;
    Ok(env)
}

pub fn load_feed(path: &Path) -> Result<Feed, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let feed: Feed = serde_yaml::from_str(&content)?;
    Ok(feed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_environment() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
name: test-env
feed: kalshi
storage:
  type: local
  path: /tmp/data
"#
        )
        .unwrap();

        let env = load_environment(file.path()).unwrap();
        assert_eq!(env.name, "test-env");
        assert_eq!(env.feed, "kalshi");
    }

    #[test]
    fn test_load_feed() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
name: kalshi
type: websocket
versions:
  - version: v1
    endpoint: wss://example.com/ws
    active: true
"#
        )
        .unwrap();

        let feed = load_feed(file.path()).unwrap();
        assert_eq!(feed.name, "kalshi");
        assert_eq!(feed.feed_type, "websocket");
        assert!(feed.active_version().is_some());
    }
}
```

**Step 2: Update lib.rs**

Add to `ssmd-connector/crates/connector/src/lib.rs`:

```rust
pub mod config;

pub use config::{load_environment, load_feed, Environment, Feed, FeedVersion, StorageConfig};
```

**Step 3: Run tests**

Run: `cd ssmd-connector && cargo test`
Expected: All tests pass

**Step 4: Commit**

```bash
git add ssmd-connector/
git commit -m "feat(connector): add YAML config loader"
```

---

## Phase 8: Binary Crate

### Task 9: Create ssmd-connector binary

**Files:**
- Create: `ssmd-connector/crates/ssmd-connector-bin/Cargo.toml`
- Create: `ssmd-connector/crates/ssmd-connector-bin/src/main.rs`
- Modify: `ssmd-connector/Cargo.toml` (add to workspace)

**Step 1: Create binary crate directory**

```bash
mkdir -p ssmd-connector/crates/ssmd-connector-bin/src
```

**Step 2: Add clap to workspace**

Add to `ssmd-connector/Cargo.toml` workspace dependencies:

```toml
clap = { version = "4", features = ["derive"] }
```

Update workspace members:

```toml
members = [
    "crates/connector",
    "crates/ssmd-connector-bin",
]
```

**Step 3: Create binary Cargo.toml**

Create `ssmd-connector/crates/ssmd-connector-bin/Cargo.toml`:

```toml
[package]
name = "ssmd-connector-bin"
version.workspace = true
edition.workspace = true

[[bin]]
name = "ssmd-connector"
path = "src/main.rs"

[dependencies]
ssmd-connector = { path = "../connector" }
tokio = { workspace = true }
clap = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

**Step 4: Create main.rs**

Create `ssmd-connector/crates/ssmd-connector-bin/src/main.rs`:

```rust
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::watch;
use tracing::{error, info};

use ssmd_connector::{
    config, EnvResolver, FileWriter, HealthState, Runner, WebSocketConnector,
    serve_health,
};

#[derive(Parser, Debug)]
#[command(name = "ssmd-connector")]
#[command(about = "Market data connector for ssmd")]
struct Args {
    /// Environment name to run
    #[arg(short, long)]
    env: String,

    /// Path to configuration directory
    #[arg(short, long)]
    config_dir: PathBuf,

    /// Health server address
    #[arg(long, default_value = "0.0.0.0:8080")]
    health_addr: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let args = Args::parse();

    // Load environment config
    let env_path = args.config_dir.join("environments").join(format!("{}.yaml", args.env));
    let env = config::load_environment(&env_path)?;
    info!(env = %env.name, "Loaded environment");

    // Load feed config
    let feed_path = args.config_dir.join("feeds").join(format!("{}.yaml", env.feed));
    let feed = config::load_feed(&feed_path)?;
    info!(feed = %feed.name, "Loaded feed");

    let version = feed.active_version()
        .ok_or("no active version for feed")?;

    // Resolve credentials
    let mut creds = None;
    if let Some(ref keys) = env.keys {
        let resolver = EnvResolver::new();
        for key_spec in keys {
            if !key_spec.source.is_empty() {
                creds = Some(resolver.resolve(&key_spec.source)?);
                break;
            }
        }
    }

    // Create components
    let connector = WebSocketConnector::new(&version.endpoint, creds);
    let writer = FileWriter::new(&env.storage.path, &feed.name);

    // Create runner
    let mut runner = Runner::new(&feed.name, connector, writer);
    let connected = runner.connected_handle();

    // Start health server
    let health_state = HealthState::new(Arc::clone(&connected));
    let health_addr: SocketAddr = args.health_addr.parse()?;

    tokio::spawn(async move {
        if let Err(e) = serve_health(health_addr, health_state).await {
            error!(error = %e, "Health server error");
        }
    });

    // Setup shutdown
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        signal::ctrl_c().await.ok();
        info!("Received shutdown signal");
        shutdown_tx.send(true).ok();
    });

    // Run collector
    info!(env = %args.env, feed = %feed.name, "Starting collector");

    if let Err(e) = runner.run(shutdown_rx).await {
        error!(error = %e, "Collector error");
        return Err(e.into());
    }

    info!("Collector stopped");
    Ok(())
}
```

**Step 5: Build binary**

Run: `cd ssmd-connector && cargo build --release`
Expected: Binary at `target/release/ssmd-connector`

**Step 6: Verify help**

Run: `./target/release/ssmd-connector --help`
Expected: Shows help with --env and --config-dir flags

**Step 7: Commit**

```bash
git add ssmd-connector/
git commit -m "feat(connector): add ssmd-connector binary"
```

---

## Phase 9: Final Verification

### Task 10: Run full test suite and verify build

**Step 1: Run all tests**

Run: `cd ssmd-connector && cargo test`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cd ssmd-connector && cargo clippy -- -D warnings`
Expected: No warnings

**Step 3: Build release binary**

Run: `cd ssmd-connector && cargo build --release`
Expected: Success

**Step 4: Verify binary runs**

Run: `./ssmd-connector/target/release/ssmd-connector --help`
Expected: Shows usage

**Step 5: Final commit if cleanup needed**

```bash
git add -A
git commit -m "chore: cleanup after Rust connector implementation"
```

---

## Summary

**Total tasks:** 10
**New crates:** connector (library), ssmd-connector-bin (binary)
**Key dependencies:** tokio, tokio-tungstenite, axum, serde, clap
**New binary:** `ssmd-connector --env <env> --config-dir <path>`
