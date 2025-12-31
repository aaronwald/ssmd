# ssmd-cdc Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Stream PostgreSQL changes to Redis via NATS for secmaster cache invalidation.

**Architecture:** Two Rust services: ssmd-cdc reads PostgreSQL WAL via wal2json and publishes to NATS JetStream; ssmd-cache consumes CDC events and updates Redis. Cache warming on startup uses LSN-anchored approach to avoid race conditions.

**Tech Stack:** Rust, tokio-postgres (with replication), async-nats, redis-rs, wal2json

---

## Task 1: Add tokio-postgres to Workspace Dependencies

**Files:**
- Modify: `ssmd-rust/Cargo.toml`

**Step 1: Add dependencies to workspace**

Add these to `[workspace.dependencies]` section:

```toml
tokio-postgres = { version = "0.7", features = ["with-chrono-0_4", "with-serde_json-1"] }
postgres-types = { version = "0.2", features = ["derive"] }
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
```

**Step 2: Verify workspace compiles**

Run: `cd ssmd-rust && cargo check`
Expected: Compiles successfully (dependencies not used yet)

**Step 3: Commit**

```bash
git add ssmd-rust/Cargo.toml
git commit -m "chore: add tokio-postgres and redis to workspace deps"
```

---

## Task 2: Create ssmd-cdc Crate Scaffold

**Files:**
- Create: `ssmd-rust/crates/ssmd-cdc/Cargo.toml`
- Create: `ssmd-rust/crates/ssmd-cdc/src/main.rs`
- Create: `ssmd-rust/crates/ssmd-cdc/src/lib.rs`
- Modify: `ssmd-rust/Cargo.toml` (add to workspace members)

**Step 1: Create Cargo.toml**

```toml
[package]
name = "ssmd-cdc"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "PostgreSQL CDC to NATS publisher for SSMD"

[[bin]]
name = "ssmd-cdc"
path = "src/main.rs"

[dependencies]
tokio = { workspace = true }
tokio-postgres = { workspace = true }
async-nats = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
clap = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

**Step 2: Create lib.rs with module structure**

```rust
pub mod config;
pub mod error;
pub mod replication;
pub mod publisher;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;
```

**Step 3: Create minimal main.rs**

```rust
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "ssmd-cdc")]
#[command(about = "PostgreSQL CDC to NATS publisher")]
struct Args {
    /// PostgreSQL connection string
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// NATS server URL
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    /// Replication slot name
    #[arg(long, env = "REPLICATION_SLOT", default_value = "ssmd_cdc")]
    slot_name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    tracing::info!(database_url = %args.database_url, nats_url = %args.nats_url, "Starting ssmd-cdc");

    // TODO: Implement CDC loop
    Ok(())
}
```

**Step 4: Add to workspace members**

In `ssmd-rust/Cargo.toml`, add `"crates/ssmd-cdc"` to members array.

**Step 5: Verify compiles**

Run: `cd ssmd-rust && cargo check -p ssmd-cdc`
Expected: Compiles (with warnings about unused)

**Step 6: Commit**

```bash
git add ssmd-rust/Cargo.toml ssmd-rust/crates/ssmd-cdc/
git commit -m "feat(ssmd-cdc): scaffold crate structure"
```

---

## Task 3: Implement Error Types

**Files:**
- Create: `ssmd-rust/crates/ssmd-cdc/src/error.rs`

**Step 1: Write error types**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    #[error("NATS error: {0}")]
    Nats(#[from] async_nats::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Replication error: {0}")]
    Replication(String),
}
```

**Step 2: Verify compiles**

Run: `cd ssmd-rust && cargo check -p ssmd-cdc`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add ssmd-rust/crates/ssmd-cdc/src/error.rs
git commit -m "feat(ssmd-cdc): add error types"
```

---

## Task 4: Implement Config Module

**Files:**
- Create: `ssmd-rust/crates/ssmd-cdc/src/config.rs`
- Create: `ssmd-rust/crates/ssmd-cdc/tests/config_test.rs`

**Step 1: Write failing test**

```rust
// tests/config_test.rs
use ssmd_cdc::config::Config;

#[test]
fn test_config_from_env() {
    std::env::set_var("DATABASE_URL", "postgres://user:pass@localhost/db");
    std::env::set_var("NATS_URL", "nats://localhost:4222");

    let config = Config::from_env().unwrap();

    assert_eq!(config.database_url, "postgres://user:pass@localhost/db");
    assert_eq!(config.nats_url, "nats://localhost:4222");
    assert_eq!(config.slot_name, "ssmd_cdc"); // default

    std::env::remove_var("DATABASE_URL");
    std::env::remove_var("NATS_URL");
}

#[test]
fn test_config_tables_default() {
    std::env::set_var("DATABASE_URL", "postgres://localhost/db");

    let config = Config::from_env().unwrap();

    assert_eq!(config.tables, vec!["events", "markets", "series_fees"]);

    std::env::remove_var("DATABASE_URL");
}
```

**Step 2: Run test to verify it fails**

Run: `cd ssmd-rust && cargo test -p ssmd-cdc config`
Expected: FAIL - module not found

**Step 3: Write implementation**

```rust
// src/config.rs
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,
    pub slot_name: String,
    pub publication_name: String,
    pub tables: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| Error::Config("DATABASE_URL not set".into()))?;

        let nats_url = std::env::var("NATS_URL")
            .unwrap_or_else(|_| "nats://localhost:4222".into());

        let slot_name = std::env::var("REPLICATION_SLOT")
            .unwrap_or_else(|_| "ssmd_cdc".into());

        let publication_name = std::env::var("PUBLICATION_NAME")
            .unwrap_or_else(|_| "ssmd_cdc_pub".into());

        let tables = std::env::var("CDC_TABLES")
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
            .unwrap_or_else(|_| vec![
                "events".into(),
                "markets".into(),
                "series_fees".into(),
            ]);

        Ok(Self {
            database_url,
            nats_url,
            slot_name,
            publication_name,
            tables,
        })
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cd ssmd-rust && cargo test -p ssmd-cdc config`
Expected: PASS

**Step 5: Commit**

```bash
git add ssmd-rust/crates/ssmd-cdc/src/config.rs ssmd-rust/crates/ssmd-cdc/tests/
git commit -m "feat(ssmd-cdc): add config module"
```

---

## Task 5: Define CDC Message Types

**Files:**
- Create: `ssmd-rust/crates/ssmd-cdc/src/messages.rs`
- Modify: `ssmd-rust/crates/ssmd-cdc/src/lib.rs`

**Step 1: Write message types**

```rust
// src/messages.rs
use serde::{Deserialize, Serialize};

/// wal2json output format
#[derive(Debug, Deserialize)]
pub struct WalJsonMessage {
    pub xid: Option<u64>,
    #[serde(default)]
    pub change: Vec<WalJsonChange>,
}

#[derive(Debug, Deserialize)]
pub struct WalJsonChange {
    pub kind: String,        // "insert", "update", "delete"
    pub schema: String,      // "public"
    pub table: String,       // "markets"
    #[serde(default)]
    pub columnnames: Vec<String>,
    #[serde(default)]
    pub columnvalues: Vec<serde_json::Value>,
    #[serde(default)]
    pub oldkeys: Option<OldKeys>,
}

#[derive(Debug, Deserialize)]
pub struct OldKeys {
    #[serde(default)]
    pub keynames: Vec<String>,
    #[serde(default)]
    pub keyvalues: Vec<serde_json::Value>,
}

/// Published CDC event (to NATS)
#[derive(Debug, Serialize, Deserialize)]
pub struct CdcEvent {
    pub lsn: String,
    pub table: String,
    pub op: CdcOperation,
    pub key: serde_json::Value,
    pub data: Option<serde_json::Value>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CdcOperation {
    Insert,
    Update,
    Delete,
}

impl CdcOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            CdcOperation::Insert => "insert",
            CdcOperation::Update => "update",
            CdcOperation::Delete => "delete",
        }
    }
}
```

**Step 2: Add to lib.rs**

Add `pub mod messages;` to lib.rs

**Step 3: Write tests for parsing**

```rust
// At bottom of messages.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wal2json_insert() {
        let json = r#"{
            "xid": 12345,
            "change": [{
                "kind": "insert",
                "schema": "public",
                "table": "markets",
                "columnnames": ["ticker", "status"],
                "columnvalues": ["INXD-25-B4000", "active"]
            }]
        }"#;

        let msg: WalJsonMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.change.len(), 1);
        assert_eq!(msg.change[0].kind, "insert");
        assert_eq!(msg.change[0].table, "markets");
    }

    #[test]
    fn test_cdc_operation_serialization() {
        let event = CdcEvent {
            lsn: "0/16B3748".into(),
            table: "markets".into(),
            op: CdcOperation::Update,
            key: serde_json::json!({"ticker": "INXD-25-B4000"}),
            data: Some(serde_json::json!({"status": "active"})),
            timestamp: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"op\":\"update\""));
    }
}
```

**Step 4: Run tests**

Run: `cd ssmd-rust && cargo test -p ssmd-cdc messages`
Expected: PASS

**Step 5: Commit**

```bash
git add ssmd-rust/crates/ssmd-cdc/src/messages.rs ssmd-rust/crates/ssmd-cdc/src/lib.rs
git commit -m "feat(ssmd-cdc): add CDC message types"
```

---

## Task 6: Implement NATS Publisher

**Files:**
- Create: `ssmd-rust/crates/ssmd-cdc/src/publisher.rs`

**Step 1: Write publisher module**

```rust
// src/publisher.rs
use async_nats::jetstream::{self, Context};
use crate::{Error, Result, messages::CdcEvent};

pub struct Publisher {
    js: Context,
    stream_name: String,
}

impl Publisher {
    pub async fn new(nats_url: &str, stream_name: &str) -> Result<Self> {
        let client = async_nats::connect(nats_url).await?;
        let js = jetstream::new(client);

        Ok(Self {
            js,
            stream_name: stream_name.to_string(),
        })
    }

    /// Ensure the CDC stream exists
    pub async fn ensure_stream(&self) -> Result<()> {
        let config = jetstream::stream::Config {
            name: self.stream_name.clone(),
            subjects: vec!["cdc.>".into()],
            max_messages: 100_000,
            max_age: std::time::Duration::from_secs(7 * 24 * 60 * 60), // 7 days
            storage: jetstream::stream::StorageType::File,
            ..Default::default()
        };

        match self.js.get_stream(&self.stream_name).await {
            Ok(_) => {
                tracing::info!(stream = %self.stream_name, "Stream already exists");
            }
            Err(_) => {
                self.js.create_stream(config).await?;
                tracing::info!(stream = %self.stream_name, "Created stream");
            }
        }

        Ok(())
    }

    /// Publish a CDC event
    pub async fn publish(&self, event: &CdcEvent) -> Result<()> {
        let subject = format!("cdc.{}.{}", event.table, event.op.as_str());
        let payload = serde_json::to_vec(event)?;

        self.js.publish(subject.clone(), payload.into()).await?.await?;

        tracing::debug!(subject = %subject, table = %event.table, "Published CDC event");
        Ok(())
    }
}
```

**Step 2: Add to lib.rs**

Add `pub mod publisher;` to lib.rs

**Step 3: Verify compiles**

Run: `cd ssmd-rust && cargo check -p ssmd-cdc`
Expected: Compiles

**Step 4: Commit**

```bash
git add ssmd-rust/crates/ssmd-cdc/src/publisher.rs ssmd-rust/crates/ssmd-cdc/src/lib.rs
git commit -m "feat(ssmd-cdc): add NATS JetStream publisher"
```

---

## Task 7: Implement Replication Slot Manager

**Files:**
- Create: `ssmd-rust/crates/ssmd-cdc/src/replication.rs`

**Step 1: Write replication module**

```rust
// src/replication.rs
use tokio_postgres::{Client, NoTls};
use crate::{Error, Result, messages::{WalJsonMessage, CdcEvent, CdcOperation}};

pub struct ReplicationSlot {
    client: Client,
    slot_name: String,
    publication_name: String,
}

impl ReplicationSlot {
    /// Connect to PostgreSQL with replication enabled
    pub async fn connect(database_url: &str, slot_name: &str, publication_name: &str) -> Result<Self> {
        // Add replication=database parameter for logical replication
        let url = if database_url.contains('?') {
            format!("{}&replication=database", database_url)
        } else {
            format!("{}?replication=database", database_url)
        };

        let (client, connection) = tokio_postgres::connect(&url, NoTls).await?;

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "PostgreSQL connection error");
            }
        });

        Ok(Self {
            client,
            slot_name: slot_name.to_string(),
            publication_name: publication_name.to_string(),
        })
    }

    /// Ensure replication slot exists
    pub async fn ensure_slot(&self) -> Result<()> {
        let exists = self.client
            .query_opt(
                "SELECT 1 FROM pg_replication_slots WHERE slot_name = $1",
                &[&self.slot_name],
            )
            .await?
            .is_some();

        if !exists {
            self.client
                .execute(
                    "SELECT pg_create_logical_replication_slot($1, 'wal2json')",
                    &[&self.slot_name],
                )
                .await?;
            tracing::info!(slot = %self.slot_name, "Created replication slot");
        } else {
            tracing::info!(slot = %self.slot_name, "Replication slot exists");
        }

        Ok(())
    }

    /// Get current WAL LSN
    pub async fn current_lsn(&self) -> Result<String> {
        let row = self.client
            .query_one("SELECT pg_current_wal_lsn()::text", &[])
            .await?;
        Ok(row.get(0))
    }

    /// Poll for changes from the replication slot
    pub async fn poll_changes(&self) -> Result<Vec<CdcEvent>> {
        let rows = self.client
            .query(
                "SELECT lsn::text, data FROM pg_logical_slot_get_changes($1, NULL, NULL,
                    'include-lsn', '1',
                    'include-timestamp', '1')",
                &[&self.slot_name],
            )
            .await?;

        let mut events = Vec::new();

        for row in rows {
            let lsn: String = row.get(0);
            let data: String = row.get(1);

            if let Ok(msg) = serde_json::from_str::<WalJsonMessage>(&data) {
                for change in msg.change {
                    let op = match change.kind.as_str() {
                        "insert" => CdcOperation::Insert,
                        "update" => CdcOperation::Update,
                        "delete" => CdcOperation::Delete,
                        _ => continue,
                    };

                    // Build key from primary key columns (first column assumed to be PK)
                    let key = if !change.columnnames.is_empty() && !change.columnvalues.is_empty() {
                        serde_json::json!({ &change.columnnames[0]: &change.columnvalues[0] })
                    } else if let Some(ref old) = change.oldkeys {
                        if !old.keynames.is_empty() && !old.keyvalues.is_empty() {
                            serde_json::json!({ &old.keynames[0]: &old.keyvalues[0] })
                        } else {
                            serde_json::Value::Null
                        }
                    } else {
                        serde_json::Value::Null
                    };

                    // Build data object from columns
                    let data = if change.columnnames.len() == change.columnvalues.len() {
                        let obj: serde_json::Map<String, serde_json::Value> = change.columnnames
                            .iter()
                            .zip(change.columnvalues.iter())
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        Some(serde_json::Value::Object(obj))
                    } else {
                        None
                    };

                    events.push(CdcEvent {
                        lsn: lsn.clone(),
                        table: change.table,
                        op,
                        key,
                        data,
                        timestamp: chrono::Utc::now(),
                    });
                }
            }
        }

        Ok(events)
    }
}
```

**Step 2: Add to lib.rs**

Add `pub mod replication;` to lib.rs

**Step 3: Verify compiles**

Run: `cd ssmd-rust && cargo check -p ssmd-cdc`
Expected: Compiles

**Step 4: Commit**

```bash
git add ssmd-rust/crates/ssmd-cdc/src/replication.rs ssmd-rust/crates/ssmd-cdc/src/lib.rs
git commit -m "feat(ssmd-cdc): add PostgreSQL replication slot manager"
```

---

## Task 8: Implement Main CDC Loop

**Files:**
- Modify: `ssmd-rust/crates/ssmd-cdc/src/main.rs`

**Step 1: Update main.rs with full implementation**

```rust
use clap::Parser;
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_cdc::{config::Config, publisher::Publisher, replication::ReplicationSlot};

#[derive(Parser)]
#[command(name = "ssmd-cdc")]
#[command(about = "PostgreSQL CDC to NATS publisher")]
struct Args {
    /// Poll interval in milliseconds
    #[arg(long, env = "POLL_INTERVAL_MS", default_value = "100")]
    poll_interval_ms: u64,

    /// NATS stream name
    #[arg(long, env = "NATS_STREAM", default_value = "SECMASTER_CDC")]
    stream_name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let config = Config::from_env()?;

    tracing::info!(
        database_url = %config.database_url.split('@').last().unwrap_or("***"),
        nats_url = %config.nats_url,
        slot = %config.slot_name,
        "Starting ssmd-cdc"
    );

    // Connect to NATS and ensure stream exists
    let publisher = Publisher::new(&config.nats_url, &args.stream_name).await?;
    publisher.ensure_stream().await?;

    // Connect to PostgreSQL and ensure replication slot exists
    let replication = ReplicationSlot::connect(
        &config.database_url,
        &config.slot_name,
        &config.publication_name,
    ).await?;
    replication.ensure_slot().await?;

    let lsn = replication.current_lsn().await?;
    tracing::info!(lsn = %lsn, "Starting from LSN");

    // Main polling loop
    let poll_interval = Duration::from_millis(args.poll_interval_ms);
    let mut events_published: u64 = 0;

    loop {
        match replication.poll_changes().await {
            Ok(events) => {
                for event in events {
                    if let Err(e) = publisher.publish(&event).await {
                        tracing::error!(error = %e, table = %event.table, "Failed to publish event");
                    } else {
                        events_published += 1;
                        if events_published % 100 == 0 {
                            tracing::info!(total = events_published, "Events published");
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to poll changes");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}
```

**Step 2: Add anyhow dependency to Cargo.toml**

Add `anyhow = "1"` to dependencies.

**Step 3: Verify compiles**

Run: `cd ssmd-rust && cargo build -p ssmd-cdc`
Expected: Compiles successfully

**Step 4: Commit**

```bash
git add ssmd-rust/crates/ssmd-cdc/
git commit -m "feat(ssmd-cdc): implement main CDC polling loop"
```

---

## Task 9: Create ssmd-cache Crate Scaffold

**Files:**
- Create: `ssmd-rust/crates/ssmd-cache/Cargo.toml`
- Create: `ssmd-rust/crates/ssmd-cache/src/main.rs`
- Create: `ssmd-rust/crates/ssmd-cache/src/lib.rs`
- Modify: `ssmd-rust/Cargo.toml`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "ssmd-cache"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "NATS CDC to Redis cache updater for SSMD"

[[bin]]
name = "ssmd-cache"
path = "src/main.rs"

[dependencies]
tokio = { workspace = true }
tokio-postgres = { workspace = true }
async-nats = { workspace = true }
redis = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
clap = { workspace = true }
futures-util = { workspace = true }
anyhow = "1"

[dev-dependencies]
tempfile = { workspace = true }
```

**Step 2: Create lib.rs**

```rust
pub mod config;
pub mod error;
pub mod cache;
pub mod warmer;
pub mod consumer;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;
```

**Step 3: Create minimal main.rs**

```rust
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "ssmd-cache")]
#[command(about = "NATS CDC to Redis cache updater")]
struct Args {
    /// PostgreSQL connection string (for cache warming)
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// NATS server URL
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    /// Redis URL
    #[arg(long, env = "REDIS_URL", default_value = "redis://localhost:6379")]
    redis_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    tracing::info!(
        nats_url = %args.nats_url,
        redis_url = %args.redis_url,
        "Starting ssmd-cache"
    );

    // TODO: Implement cache warming and CDC consumption
    Ok(())
}
```

**Step 4: Add to workspace members**

Add `"crates/ssmd-cache"` to members in `ssmd-rust/Cargo.toml`.

**Step 5: Verify compiles**

Run: `cd ssmd-rust && cargo check -p ssmd-cache`
Expected: Compiles

**Step 6: Commit**

```bash
git add ssmd-rust/Cargo.toml ssmd-rust/crates/ssmd-cache/
git commit -m "feat(ssmd-cache): scaffold crate structure"
```

---

## Task 10: Implement ssmd-cache Error and Config

**Files:**
- Create: `ssmd-rust/crates/ssmd-cache/src/error.rs`
- Create: `ssmd-rust/crates/ssmd-cache/src/config.rs`

**Step 1: Write error.rs**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    #[error("NATS error: {0}")]
    Nats(#[from] async_nats::Error),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(String),
}
```

**Step 2: Write config.rs**

```rust
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,
    pub redis_url: String,
    pub stream_name: String,
    pub consumer_name: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| Error::Config("DATABASE_URL not set".into()))?;

        let nats_url = std::env::var("NATS_URL")
            .unwrap_or_else(|_| "nats://localhost:4222".into());

        let redis_url = std::env::var("REDIS_URL")
            .unwrap_or_else(|_| "redis://localhost:6379".into());

        let stream_name = std::env::var("NATS_STREAM")
            .unwrap_or_else(|_| "SECMASTER_CDC".into());

        let consumer_name = std::env::var("CONSUMER_NAME")
            .unwrap_or_else(|_| "ssmd-cache".into());

        Ok(Self {
            database_url,
            nats_url,
            redis_url,
            stream_name,
            consumer_name,
        })
    }
}
```

**Step 3: Verify compiles**

Run: `cd ssmd-rust && cargo check -p ssmd-cache`
Expected: Compiles

**Step 4: Commit**

```bash
git add ssmd-rust/crates/ssmd-cache/src/
git commit -m "feat(ssmd-cache): add error and config modules"
```

---

## Task 11: Implement Redis Cache Module

**Files:**
- Create: `ssmd-rust/crates/ssmd-cache/src/cache.rs`

**Step 1: Write cache module**

```rust
use redis::AsyncCommands;
use crate::Result;

pub struct RedisCache {
    client: redis::Client,
}

impl RedisCache {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;

        // Test connection
        let mut conn = client.get_multiplexed_async_connection().await?;
        let _: String = redis::cmd("PING").query_async(&mut conn).await?;

        tracing::info!("Connected to Redis");
        Ok(Self { client })
    }

    async fn conn(&self) -> Result<redis::aio::MultiplexedConnection> {
        Ok(self.client.get_multiplexed_async_connection().await?)
    }

    /// Set a secmaster record
    pub async fn set(&self, table: &str, key: &str, value: &serde_json::Value) -> Result<()> {
        let redis_key = format!("secmaster:{}:{}", table, key);
        let json = serde_json::to_string(value)?;

        let mut conn = self.conn().await?;
        conn.set::<_, _, ()>(&redis_key, &json).await?;

        tracing::debug!(key = %redis_key, "SET");
        Ok(())
    }

    /// Delete a secmaster record
    pub async fn delete(&self, table: &str, key: &str) -> Result<()> {
        let redis_key = format!("secmaster:{}:{}", table, key);

        let mut conn = self.conn().await?;
        conn.del::<_, ()>(&redis_key).await?;

        tracing::debug!(key = %redis_key, "DEL");
        Ok(())
    }

    /// Get count of secmaster keys
    pub async fn count(&self, table: &str) -> Result<u64> {
        let pattern = format!("secmaster:{}:*", table);

        let mut conn = self.conn().await?;
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await?;

        Ok(keys.len() as u64)
    }
}
```

**Step 2: Verify compiles**

Run: `cd ssmd-rust && cargo check -p ssmd-cache`
Expected: Compiles

**Step 3: Commit**

```bash
git add ssmd-rust/crates/ssmd-cache/src/cache.rs
git commit -m "feat(ssmd-cache): add Redis cache module"
```

---

## Task 12: Implement Cache Warmer

**Files:**
- Create: `ssmd-rust/crates/ssmd-cache/src/warmer.rs`

**Step 1: Write warmer module**

```rust
use tokio_postgres::{Client, NoTls};
use crate::{Result, cache::RedisCache};

pub struct CacheWarmer {
    client: Client,
}

impl CacheWarmer {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "PostgreSQL connection error");
            }
        });

        Ok(Self { client })
    }

    /// Get current WAL LSN (for race condition handling)
    pub async fn current_lsn(&self) -> Result<String> {
        let row = self.client
            .query_one("SELECT pg_current_wal_lsn()::text", &[])
            .await?;
        Ok(row.get(0))
    }

    /// Warm markets table into Redis
    pub async fn warm_markets(&self, cache: &RedisCache) -> Result<u64> {
        let rows = self.client
            .query("SELECT ticker, row_to_json(markets.*) FROM markets", &[])
            .await?;

        let mut count = 0;
        for row in rows {
            let ticker: String = row.get(0);
            let json: serde_json::Value = row.get(1);
            cache.set("market", &ticker, &json).await?;
            count += 1;
        }

        tracing::info!(count, "Warmed markets");
        Ok(count)
    }

    /// Warm events table into Redis
    pub async fn warm_events(&self, cache: &RedisCache) -> Result<u64> {
        let rows = self.client
            .query("SELECT event_ticker, row_to_json(events.*) FROM events", &[])
            .await?;

        let mut count = 0;
        for row in rows {
            let event_ticker: String = row.get(0);
            let json: serde_json::Value = row.get(1);
            cache.set("event", &event_ticker, &json).await?;
            count += 1;
        }

        tracing::info!(count, "Warmed events");
        Ok(count)
    }

    /// Warm series_fees table into Redis
    pub async fn warm_fees(&self, cache: &RedisCache) -> Result<u64> {
        let rows = self.client
            .query("SELECT series_ticker, row_to_json(series_fees.*) FROM series_fees", &[])
            .await?;

        let mut count = 0;
        for row in rows {
            let series_ticker: String = row.get(0);
            let json: serde_json::Value = row.get(1);
            cache.set("fee", &series_ticker, &json).await?;
            count += 1;
        }

        tracing::info!(count, "Warmed fees");
        Ok(count)
    }

    /// Warm all tables
    pub async fn warm_all(&self, cache: &RedisCache) -> Result<String> {
        let start = std::time::Instant::now();

        // Get LSN before warming
        let lsn = self.current_lsn().await?;
        tracing::info!(lsn = %lsn, "Snapshot LSN");

        // Warm each table
        let markets = self.warm_markets(cache).await?;
        let events = self.warm_events(cache).await?;
        let fees = self.warm_fees(cache).await?;

        let elapsed = start.elapsed();
        tracing::info!(
            markets,
            events,
            fees,
            elapsed_ms = elapsed.as_millis(),
            "Cache warming complete"
        );

        Ok(lsn)
    }
}
```

**Step 2: Verify compiles**

Run: `cd ssmd-rust && cargo check -p ssmd-cache`
Expected: Compiles

**Step 3: Commit**

```bash
git add ssmd-rust/crates/ssmd-cache/src/warmer.rs
git commit -m "feat(ssmd-cache): add cache warmer module"
```

---

## Task 13: Implement CDC Consumer

**Files:**
- Create: `ssmd-rust/crates/ssmd-cache/src/consumer.rs`

**Step 1: Write consumer module**

```rust
use async_nats::jetstream::{self, consumer::pull::Stream, Context};
use futures_util::StreamExt;
use crate::{Result, cache::RedisCache};

/// CDC event from NATS (matches ssmd-cdc publisher format)
#[derive(Debug, serde::Deserialize)]
pub struct CdcEvent {
    pub lsn: String,
    pub table: String,
    pub op: String,  // "insert", "update", "delete"
    pub key: serde_json::Value,
    pub data: Option<serde_json::Value>,
}

pub struct CdcConsumer {
    stream: Stream,
    snapshot_lsn: String,
}

impl CdcConsumer {
    pub async fn new(
        nats_url: &str,
        stream_name: &str,
        consumer_name: &str,
        snapshot_lsn: String,
    ) -> Result<Self> {
        let client = async_nats::connect(nats_url).await?;
        let js: Context = jetstream::new(client);

        // Get or create consumer
        let stream_obj = js.get_stream(stream_name).await?;

        let consumer = stream_obj
            .get_or_create_consumer(
                consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(consumer_name.to_string()),
                    filter_subject: "cdc.>".to_string(),
                    ..Default::default()
                },
            )
            .await?;

        let messages = consumer.messages().await?;

        Ok(Self {
            stream: messages,
            snapshot_lsn,
        })
    }

    /// Compare LSNs (format: "0/16B3748")
    fn lsn_gte(&self, lsn: &str, threshold: &str) -> bool {
        // Simple string comparison works for LSN format
        lsn >= threshold
    }

    /// Process CDC events and update cache
    pub async fn run(&mut self, cache: &RedisCache) -> Result<()> {
        tracing::info!(snapshot_lsn = %self.snapshot_lsn, "Starting CDC consumer");

        let mut processed: u64 = 0;
        let mut skipped: u64 = 0;

        while let Some(msg) = self.stream.next().await {
            let msg = msg?;

            match serde_json::from_slice::<CdcEvent>(&msg.payload) {
                Ok(event) => {
                    // Skip events before snapshot LSN
                    if !self.lsn_gte(&event.lsn, &self.snapshot_lsn) {
                        skipped += 1;
                        msg.ack().await?;
                        continue;
                    }

                    // Extract key (assumes first field is the key)
                    let key = match &event.key {
                        serde_json::Value::Object(obj) => {
                            obj.values().next()
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        }
                        _ => None,
                    };

                    if let Some(key) = key {
                        match event.op.as_str() {
                            "insert" | "update" => {
                                if let Some(data) = &event.data {
                                    cache.set(&event.table, &key, data).await?;
                                }
                            }
                            "delete" => {
                                cache.delete(&event.table, &key).await?;
                            }
                            _ => {}
                        }
                    }

                    processed += 1;
                    if processed % 100 == 0 {
                        tracing::info!(processed, skipped, "CDC events processed");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to parse CDC event");
                }
            }

            msg.ack().await?;
        }

        Ok(())
    }
}
```

**Step 2: Verify compiles**

Run: `cd ssmd-rust && cargo check -p ssmd-cache`
Expected: Compiles

**Step 3: Commit**

```bash
git add ssmd-rust/crates/ssmd-cache/src/consumer.rs
git commit -m "feat(ssmd-cache): add CDC consumer module"
```

---

## Task 14: Complete ssmd-cache Main

**Files:**
- Modify: `ssmd-rust/crates/ssmd-cache/src/main.rs`

**Step 1: Update main.rs**

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_cache::{
    config::Config,
    cache::RedisCache,
    warmer::CacheWarmer,
    consumer::CdcConsumer,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env()?;

    tracing::info!(
        redis_url = %config.redis_url,
        nats_url = %config.nats_url,
        stream = %config.stream_name,
        "Starting ssmd-cache"
    );

    // Connect to Redis
    let cache = RedisCache::new(&config.redis_url).await?;

    // Connect to PostgreSQL and warm cache
    let warmer = CacheWarmer::connect(&config.database_url).await?;
    let snapshot_lsn = warmer.warm_all(&cache).await?;

    // Log cache stats
    let markets = cache.count("market").await?;
    let events = cache.count("event").await?;
    let fees = cache.count("fee").await?;
    tracing::info!(markets, events, fees, "Cache populated");

    // Start consuming CDC events
    let mut consumer = CdcConsumer::new(
        &config.nats_url,
        &config.stream_name,
        &config.consumer_name,
        snapshot_lsn,
    ).await?;

    consumer.run(&cache).await?;

    Ok(())
}
```

**Step 2: Verify compiles**

Run: `cd ssmd-rust && cargo build -p ssmd-cache`
Expected: Compiles

**Step 3: Commit**

```bash
git add ssmd-rust/crates/ssmd-cache/
git commit -m "feat(ssmd-cache): complete main with warming and CDC consumption"
```

---

## Task 15: Add Dockerfiles

**Files:**
- Create: `ssmd-rust/crates/ssmd-cdc/Dockerfile`
- Create: `ssmd-rust/crates/ssmd-cache/Dockerfile`

**Step 1: Create ssmd-cdc Dockerfile**

```dockerfile
FROM rust:1.83-slim-bookworm as builder

WORKDIR /app
COPY . .
RUN cargo build --release -p ssmd-cdc

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ssmd-cdc /usr/local/bin/
ENTRYPOINT ["ssmd-cdc"]
```

**Step 2: Create ssmd-cache Dockerfile**

```dockerfile
FROM rust:1.83-slim-bookworm as builder

WORKDIR /app
COPY . .
RUN cargo build --release -p ssmd-cache

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ssmd-cache /usr/local/bin/
ENTRYPOINT ["ssmd-cache"]
```

**Step 3: Commit**

```bash
git add ssmd-rust/crates/ssmd-cdc/Dockerfile ssmd-rust/crates/ssmd-cache/Dockerfile
git commit -m "feat: add Dockerfiles for ssmd-cdc and ssmd-cache"
```

---

## Task 16: Add GitHub Actions Workflows

**Files:**
- Create: `.github/workflows/build-ssmd-cdc.yaml`
- Create: `.github/workflows/build-ssmd-cache.yaml`

**Step 1: Create ssmd-cdc workflow**

```yaml
name: Build ssmd-cdc

on:
  push:
    tags:
      - 'ssmd-cdc-v*'

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository_owner }}/ssmd-cdc

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

      - name: Log in to Container Registry
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Extract version from tag
        id: version
        run: echo "VERSION=${GITHUB_REF_NAME#ssmd-cdc-v}" >> $GITHUB_OUTPUT

      - name: Build and push
        uses: docker/build-push-action@v5
        with:
          context: ./ssmd-rust
          file: ./ssmd-rust/crates/ssmd-cdc/Dockerfile
          push: true
          tags: |
            ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}:${{ steps.version.outputs.VERSION }}
            ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}:latest
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

**Step 2: Create ssmd-cache workflow**

```yaml
name: Build ssmd-cache

on:
  push:
    tags:
      - 'ssmd-cache-v*'

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository_owner }}/ssmd-cache

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

      - name: Log in to Container Registry
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Extract version from tag
        id: version
        run: echo "VERSION=${GITHUB_REF_NAME#ssmd-cache-v}" >> $GITHUB_OUTPUT

      - name: Build and push
        uses: docker/build-push-action@v5
        with:
          context: ./ssmd-rust
          file: ./ssmd-rust/crates/ssmd-cache/Dockerfile
          push: true
          tags: |
            ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}:${{ steps.version.outputs.VERSION }}
            ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}:latest
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

**Step 3: Commit**

```bash
git add .github/workflows/
git commit -m "ci: add GitHub Actions for ssmd-cdc and ssmd-cache builds"
```

---

## Task 17: Run Full Build and Tests

**Step 1: Run all Rust tests**

Run: `cd ssmd-rust && cargo test --all`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cd ssmd-rust && cargo clippy --all`
Expected: No errors (warnings OK)

**Step 3: Build release**

Run: `cd ssmd-rust && cargo build --release -p ssmd-cdc -p ssmd-cache`
Expected: Builds successfully

**Step 4: Commit any fixes**

If clippy suggested fixes, apply and commit them.

---

## Task 18: Merge to Main

**Step 1: Push feature branch**

```bash
git push origin feature/ssmd-cdc
```

**Step 2: Merge to main**

```bash
git checkout main
git merge feature/ssmd-cdc --no-edit
git push origin main
```

**Step 3: Clean up worktree**

```bash
git worktree remove .worktrees/ssmd-cdc
git branch -d feature/ssmd-cdc
```

---

## Summary

This plan creates:
- **ssmd-cdc**: Rust service (~400 lines) that reads PostgreSQL WAL via wal2json and publishes to NATS
- **ssmd-cache**: Rust service (~350 lines) that warms cache from PostgreSQL and consumes CDC events to update Redis
- **Dockerfiles** and **GitHub Actions** for CI/CD

Next steps after merge:
1. Install wal2json on ssmd-postgres
2. Deploy ssmd-redis to cluster
3. Tag and deploy ssmd-cdc and ssmd-cache
