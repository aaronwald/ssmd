# Runtime Framework Design

## Overview

Add a runtime framework (ssmd-connector) in Rust that reads the GitOps metadata (feeds, schemas, environments) and actually collects market data. Proves the metadata model works end-to-end.

**Note:** Per the original Kalshi design (`docs/plans/kalshi/01-overview.md`), runtime components are in Rust while the ssmd CLI (Go) handles metadata/environment management only.

## Command

```
ssmd-connector --env <environment> --config-dir <path>
```

- `--config-dir` is required (no default)
- Reads environment config, loads referenced feed, resolves keys from env vars
- Connects to data source, writes raw messages to local storage
- Exposes health/metrics HTTP endpoints
- Exits on disconnect (K8s handles restart)

## Architecture

```
┌─────────────────────────────────────────────────┐
│          ssmd-connector kalshi-dev              │
├─────────────────────────────────────────────────┤
│  Config Loader                                  │
│  - Reads environments/<env>.yaml                │
│  - Loads referenced feed                        │
│  - Resolves keys from env vars                  │
├─────────────────────────────────────────────────┤
│  Connector (trait)                              │
│  - WebSocketConnector for Kalshi                │
│  - Connects, authenticates, receives messages   │
│  - On disconnect: exit process                  │
├─────────────────────────────────────────────────┤
│  Writer (trait)                                 │
│  - FileWriter for JSONL                         │
│  - Date-partitioned files                       │
├─────────────────────────────────────────────────┤
│  HTTP Server (:8080)                            │
│  - GET /health - liveness probe                 │
│  - GET /ready  - readiness probe                │
│  - GET /metrics - Prometheus format             │
└─────────────────────────────────────────────────┘
```

## Framework Traits (Rust)

```rust
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Connector trait - WebSocketConnector, RESTPoller, etc.
#[async_trait]
pub trait Connector: Send + Sync {
    async fn connect(&mut self) -> Result<(), ConnectorError>;
    fn messages(&self) -> mpsc::Receiver<Vec<u8>>;
    async fn close(&mut self) -> Result<(), ConnectorError>;
}

/// Writer trait - FileWriter, S3Writer, etc.
#[async_trait]
pub trait Writer: Send + Sync {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError>;
    async fn close(&mut self) -> Result<(), WriterError>;
}

/// KeyResolver trait - EnvResolver, VaultResolver, etc.
pub trait KeyResolver: Send + Sync {
    fn resolve(&self, source: &str) -> Result<HashMap<String, String>, ResolverError>;
}

/// Message wraps raw data with metadata
pub struct Message {
    pub timestamp: String,
    pub feed: String,
    pub data: Vec<u8>,
}
```

## Package Structure (Cargo Workspace)

```
ssmd-rust/
  Cargo.toml              # Workspace root
  crates/
    metadata/             # Shared metadata types (mirrors Go types)
      src/
        lib.rs
        feed.rs           # Feed, FeedVersion, FeedType, FeedStatus, etc.
        environment.rs    # Environment, KeySpec, StorageConfig, etc.
        schema.rs         # Schema types
        version.rs        # Version resolution helpers
    connector/            # Core runtime library
      src/
        lib.rs
        traits.rs         # Connector, Writer, KeyResolver traits
        runner.rs         # Main run loop
        websocket.rs      # WebSocket connector implementation
        writer.rs         # FileWriter for JSONL
        resolver.rs       # Environment variable resolver
        server.rs         # Health/metrics HTTP server
    ssmd-connector/       # Binary crate
      src/
        main.rs           # CLI entry point
```

## Metadata Crate (ssmd-metadata)

The `metadata` crate mirrors the Go types in `internal/types/` to ensure Rust components can read the same YAML configs.

**Feed Types:**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FeedType {
    Websocket,
    Rest,
    Multicast,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FeedStatus {
    Active,
    Deprecated,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    ApiKey,
    Oauth,
    Mtls,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feed {
    pub name: String,
    pub display_name: Option<String>,
    #[serde(rename = "type")]
    pub feed_type: FeedType,
    #[serde(default)]
    pub status: Option<FeedStatus>,
    pub capture_locations: Option<Vec<CaptureLocation>>,
    pub versions: Vec<FeedVersion>,
    pub calendar: Option<Calendar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedVersion {
    pub version: String,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub protocol: String,
    pub endpoint: String,
    pub auth_method: Option<AuthMethod>,
    pub rate_limit_per_second: Option<i32>,
    pub max_symbols_per_connection: Option<i32>,
    pub supports_orderbook: Option<bool>,
    pub supports_trades: Option<bool>,
    pub supports_historical: Option<bool>,
    pub parser_config: Option<HashMap<String, String>>,
}
```

**Environment Types:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TransportType {
    Nats,
    Mqtt,
    Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StorageType {
    Local,
    S3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum KeyType {
    ApiKey,
    Transport,
    Storage,
    Tls,
    Webhook,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub feed: String,
    pub schema: String,
    pub schedule: Option<Schedule>,
    pub keys: Option<HashMap<String, KeySpec>>,
    pub transport: TransportConfig,
    pub storage: StorageConfig,
    pub cache: Option<CacheConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySpec {
    #[serde(rename = "type")]
    pub key_type: KeyType,
    pub description: Option<String>,
    pub required: Option<bool>,
    pub fields: Vec<String>,
    pub source: Option<String>,
    pub rotation_days: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(rename = "type")]
    pub storage_type: StorageType,
    pub path: Option<String>,
    pub bucket: Option<String>,
    pub region: Option<String>,
}
```

**Loading Functions:**

```rust
impl Feed {
    pub fn load(path: &Path) -> Result<Self, MetadataError>;
    pub fn get_version_for_date(&self, date: NaiveDate) -> Option<&FeedVersion>;
    pub fn get_latest_version(&self) -> Option<&FeedVersion>;
}

impl Environment {
    pub fn load(path: &Path) -> Result<Self, MetadataError>;
    pub fn get_schema_name(&self) -> &str;
    pub fn get_schema_version(&self) -> &str;
}
```

## Data Flow

```
Kalshi WebSocket
       │
       ▼
┌─────────────────┐
│ Connector       │ Receives raw JSON messages
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Runner          │ Adds metadata (timestamp, source)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Writer          │ Appends to JSONL file
└─────────────────┘
```

## Storage Format

**File Structure**

```
{storage.path}/
  2025-12-22/
    kalshi.jsonl
  2025-12-23/
    kalshi.jsonl
```

**JSONL Format** (one message per line)

```json
{"ts":"2025-12-22T10:30:00Z","feed":"kalshi","data":{...raw message...}}
```

## Health & Metrics

**Endpoints**

```
GET /health   → {"status":"ok"} or {"status":"error","reason":"..."}
GET /ready    → {"status":"ready"} or {"status":"not_ready"}
GET /metrics  → Prometheus format
```

**Health Logic**
- `/health` - Returns OK if process is running (liveness)
- `/ready` - Returns OK if WebSocket is connected (readiness)

**Metrics**

```
ssmd_messages_total{feed="kalshi"} 12345
ssmd_errors_total{feed="kalshi",type="write"} 0
ssmd_connected{feed="kalshi"} 1
ssmd_last_message_timestamp{feed="kalshi"} 1703245800
```

## K8s Deployment

**GitOps Flow**

- `exchanges/` directory in git → ConfigMap mounted at `/etc/ssmd/exchanges`
- Secrets (API keys) → K8s Secret → injected as env vars
- `ssmd-connector --env <env> --config-dir /etc/ssmd/exchanges`

**Manifest Example**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: kalshi-collector
spec:
  replicas: 1
  template:
    spec:
      containers:
      - name: connector
        image: ssmd-connector:latest
        args: ["--env", "kalshi-dev", "--config-dir", "/etc/ssmd/exchanges"]
        ports:
        - containerPort: 8080
        envFrom:
        - secretRef:
            name: kalshi-keys
        volumeMounts:
        - name: config
          mountPath: /etc/ssmd/exchanges
        livenessProbe:
          httpGet: {path: /health, port: 8080}
        readinessProbe:
          httpGet: {path: /ready, port: 8080}
      volumes:
      - name: config
        configMap:
          name: ssmd-config
```

## Phase 1 Scope

**Building:**
- `ssmd-metadata` crate - Rust types mirroring Go types for config parsing
- `ssmd-connector --env <env> --config-dir <path>` binary (Rust)
- Framework traits: Connector, Writer, KeyResolver
- WebSocketConnector (for Kalshi)
- FileWriter (JSONL, date-partitioned)
- EnvResolver (reads from env vars)
- HTTP server with /health, /ready, /metrics
- Fail-fast on disconnect

**Not building yet:**
- REST polling connector
- S3/Parquet writer
- Vault key resolver
- Schema validation
- NATS transport (Phase 2)
- Scheduling/rate limiting

## Tech Stack

- **Rust** - tokio async runtime
- **tokio-tungstenite** - WebSocket client
- **serde/serde_yaml** - Config parsing
- **axum** - HTTP server for health/metrics
- **clap** - CLI argument parsing
- **tracing** - Structured logging
