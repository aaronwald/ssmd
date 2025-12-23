# Kalshi Port Implementation Plan

**Date:** 2025-12-23
**Status:** Draft
**Source:** tradfiportal/crates/kalshi-*

## Overview

Port Kalshi WebSocket connector from tradfiportal to ssmd architecture. Focus on streaming trades with ssmd middleware abstractions.

## Dependencies to Add

```toml
# ssmd-rust/crates/connector/Cargo.toml
rsa = "0.9"
sha2 = "0.10"
base64 = "0.22"
rand = "0.8"
chrono = { version = "0.4", features = ["serde"] }
```

## Tasks

### Task 1: Kalshi Auth Module
**File:** `ssmd-rust/crates/connector/src/kalshi/auth.rs`

Port RSA-PSS signing from tradfiportal:
- `KalshiCredentials` struct with api_key + RsaPrivateKey
- `sign_websocket_request()` -> (timestamp, signature)
- Support both PKCS#8 and PKCS#1 PEM formats
- Message format: `{timestamp}GET/trade-api/ws/v2`

**Test:** Unit test signature generation (can't verify without Kalshi, but test format)

### Task 2: Kalshi Message Types
**File:** `ssmd-rust/crates/connector/src/kalshi/messages.rs`

Port message types from tradfiportal:
```rust
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    Subscribed { id: u64 },
    Ticker { msg: TickerData },
    Trade { msg: TradeData },
    OrderbookSnapshot { msg: OrderbookData },
    OrderbookDelta { msg: OrderbookData },
    #[serde(other)]
    Unknown,
}

pub struct TickerData {
    pub market_ticker: String,
    pub yes_bid: Option<i64>,
    pub yes_ask: Option<i64>,
    pub last_price: Option<i64>,
    pub volume: Option<i64>,
    pub ts: DateTime<Utc>,  // unix timestamp deserializer
}

pub struct TradeData {
    pub market_ticker: String,
    pub price: i64,
    pub count: i64,
    pub side: String,
    pub ts: DateTime<Utc>,
}
```

**Test:** Parse real Kalshi messages from tradfiportal test fixtures

### Task 3: Kalshi WebSocket Client
**File:** `ssmd-rust/crates/connector/src/kalshi/websocket.rs`

Implement Kalshi-specific WebSocket:
- `KalshiWebSocket::connect(credentials, use_demo)` with auth headers
- `subscribe_ticker()`, `subscribe_all_trades()`, `subscribe_orderbook(ticker)`
- `recv() -> WsMessage` with ping/pong handling
- Production URL: `wss://api.elections.kalshi.com/trade-api/ws/v2`
- Demo URL: `wss://demo-api.kalshi.co/trade-api/ws/v2`

**Test:** Mock WebSocket test for subscription flow

### Task 4: Kalshi Connector Trait Implementation
**File:** `ssmd-rust/crates/connector/src/kalshi/connector.rs`

Implement ssmd `Connector` trait:
```rust
pub struct KalshiConnector {
    credentials: KalshiCredentials,
    use_demo: bool,
    ws: Option<KalshiWebSocket>,
    tx: Option<mpsc::Sender<Vec<u8>>>,
    rx: Option<mpsc::Receiver<Vec<u8>>>,
}

#[async_trait]
impl Connector for KalshiConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError>;
    fn messages(&mut self) -> mpsc::Receiver<Vec<u8>>;
    async fn close(&mut self) -> Result<(), ConnectorError>;
}
```

Internally spawns task to:
1. Connect with auth
2. Subscribe to ticker + trades
3. Forward parsed messages to channel

**Test:** Integration test with mock server

### Task 5: Kalshi Module Export
**File:** `ssmd-rust/crates/connector/src/kalshi/mod.rs`

```rust
mod auth;
mod connector;
mod messages;
mod websocket;

pub use auth::KalshiCredentials;
pub use connector::KalshiConnector;
pub use messages::{WsMessage, TickerData, TradeData, OrderbookData};
```

**File:** `ssmd-rust/crates/connector/src/lib.rs`
Add: `pub mod kalshi;`

### Task 6: Trade Normalization
**File:** `ssmd-rust/crates/connector/src/kalshi/normalize.rs`

Convert Kalshi messages to ssmd types:
```rust
impl From<&kalshi::TradeData> for crate::message::Message {
    fn from(trade: &kalshi::TradeData) -> Self {
        Message::new(
            "kalshi",
            serde_json::json!({
                "type": "trade",
                "ticker": trade.market_ticker,
                "price": trade.price,
                "size": trade.count,
                "side": trade.side,
                "timestamp": trade.ts.timestamp_nanos_opt()
            })
        )
    }
}
```

Future: Convert to Cap'n Proto `Trade` directly

### Task 7: Config Integration
**File:** `ssmd-rust/crates/connector/src/kalshi/config.rs`

Load credentials from environment:
```rust
pub struct KalshiConfig {
    pub api_key: String,
    pub private_key_pem: String,
    pub use_demo: bool,
}

impl KalshiConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            api_key: env::var("KALSHI_API_KEY")?,
            private_key_pem: env::var("KALSHI_PRIVATE_KEY")?,
            use_demo: env::var("KALSHI_USE_DEMO")
                .map(|v| v == "true")
                .unwrap_or(false),
        })
    }
}
```

### Task 8: Binary Entry Point Update
**File:** `ssmd-rust/crates/ssmd-connector/src/main.rs`

Wire up Kalshi connector:
```rust
use ssmd_connector_lib::kalshi::{KalshiConfig, KalshiConnector, KalshiCredentials};

#[tokio::main]
async fn main() -> Result<()> {
    // Load config
    let config = KalshiConfig::from_env()?;
    let credentials = KalshiCredentials::new(
        config.api_key,
        &config.private_key_pem
    )?;

    // Create connector
    let connector = KalshiConnector::new(credentials, config.use_demo);

    // Create writer (file for now, NATS later)
    let writer = FileWriter::new("data/kalshi")?;

    // Run
    let mut runner = Runner::new("kalshi", connector, writer);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Handle SIGTERM
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        shutdown_tx.send(true).ok();
    });

    runner.run(shutdown_rx).await?;
    Ok(())
}
```

### Task 9: NATS Transport Implementation
**File:** `ssmd-rust/crates/middleware/src/nats/mod.rs`

Implement real NATS transport:
```rust
pub struct NatsTransport {
    client: async_nats::Client,
    jetstream: async_nats::jetstream::Context,
}

#[async_trait]
impl Transport for NatsTransport {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), MiddlewareError>;
    async fn subscribe(&self, subject: &str) -> Result<BoxStream<TransportMessage>, MiddlewareError>;
}
```

Add to `MiddlewareFactory`:
```rust
"nats" => {
    let url = config.transport.url.as_ref()
        .ok_or(MiddlewareError::Config("nats requires url"))?;
    let client = async_nats::connect(url).await?;
    let jetstream = async_nats::jetstream::new(client.clone());
    Arc::new(NatsTransport { client, jetstream })
}
```

### Task 10: Environment Config Update
**File:** `exchanges/environments/kalshi-dev.yaml`

```yaml
name: kalshi-dev
feed: kalshi
status: active
transport:
  type: nats
  url: nats://localhost:4222
storage:
  type: file
  path: ./data/kalshi
keys:
  - name: KALSHI_API_KEY
    type: api_key
    source: env
  - name: KALSHI_PRIVATE_KEY
    type: api_secret
    source: env
```

### Task 11: Full Test Suite
Run and fix any issues:
```bash
make rust-test
make rust-clippy
```

### Task 12: Manual Integration Test
1. Start NATS: `docker run -p 4222:4222 nats:latest`
2. Set env vars: `KALSHI_API_KEY`, `KALSHI_PRIVATE_KEY`, `KALSHI_USE_DEMO=true`
3. Run: `cargo run -p ssmd-connector`
4. Verify messages flow to NATS

## File Summary

| File | Action | Description |
|------|--------|-------------|
| `connector/src/kalshi/mod.rs` | Create | Module exports |
| `connector/src/kalshi/auth.rs` | Create | RSA-PSS auth (port) |
| `connector/src/kalshi/messages.rs` | Create | Message types (port) |
| `connector/src/kalshi/websocket.rs` | Create | WebSocket client (port) |
| `connector/src/kalshi/connector.rs` | Create | Connector trait impl |
| `connector/src/kalshi/normalize.rs` | Create | Message normalization |
| `connector/src/kalshi/config.rs` | Create | Config loading |
| `connector/src/lib.rs` | Modify | Add kalshi module |
| `connector/Cargo.toml` | Modify | Add dependencies |
| `middleware/src/nats/mod.rs` | Create | NATS transport |
| `middleware/src/lib.rs` | Modify | Add nats module |
| `middleware/src/factory.rs` | Modify | Add nats case |
| `middleware/Cargo.toml` | Modify | Add async-nats |
| `ssmd-connector/src/main.rs` | Modify | Wire up Kalshi |
| `environments/kalshi-dev.yaml` | Modify | Add NATS config |

## Acceptance Criteria

1. `make rust-test` passes with new Kalshi tests
2. `make rust-clippy` passes
3. Can connect to Kalshi demo API with valid credentials
4. Trades flow through pipeline to NATS (or file writer)
5. Graceful shutdown on SIGTERM

## Out of Scope

- Secmaster sync (events/markets) - defer to later
- Orderbook state management - just pass through deltas
- Multi-tenant per-user connections - ssmd is single-feed
- Redis activity buckets - use NATS + S3 archival
