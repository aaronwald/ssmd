# ssmd: Stupid Simple Market Data - Kalshi Design

## Overview

ssmd is a homelab-friendly market data system. It captures live market data, streams it for real-time consumption, and archives it for backtesting. Simple enough to admin via TUI, simple enough for AI agents to query.

**First milestone:** End-to-end Kalshi streaming in 3-4 weeks.

## Goals

- Capture live Kalshi data and stream to clients (AI agent, trading bot, TUI)
- Archive raw and normalized data to S3-compatible storage
- Daily teardown/startup cycle - no long-running state
- Learn Rust and Cap'n Proto on a real project

## Non-Goals

- Ultra-low-latency (Kalshi is WebSocket, not binary multicast)
- Multiple markets (Polymarket, Kraken come later)
- Complex routing or tickerplant functionality

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Kalshi    │────▶│  Connector  │────▶│    NATS     │
│  WebSocket  │     │   (Rust)    │     │  JetStream  │
└─────────────┘     └──────┬──────┘     └──────┬──────┘
                           │                   │
                           ▼                   ├────────────────┐
                    ┌─────────────┐            │                │
                    │ Raw Archive │            ▼                ▼
                    │   (JSONL)   │     ┌─────────────┐  ┌─────────────┐
                    └──────┬──────┘     │  Archiver   │  │   Gateway   │
                           │            │   (Rust)    │  │   (Rust)    │
                           ▼            └──────┬──────┘  └──────┬──────┘
                    ┌─────────────┐            │                │
                    │     S3      │◀───────────┘                │
                    │ (raw/norm)  │                             ▼
                    └─────────────┘                      ┌─────────────┐
                                                         │   Clients   │
                                                         │ (WS + JSON) │
                                                         └─────────────┘
```

## Components

| Component | Language | Purpose |
|-----------|----------|---------|
| ssmd-connector | Rust | Kalshi WebSocket → NATS (Cap'n Proto) |
| ssmd-archiver | Rust | NATS → S3 normalized storage |
| ssmd-gateway | Rust | NATS → WebSocket (JSON for clients) |
| ssmd-cli | Go | Environment management, operations |
| ssmd-worker | Go | Temporal workflows for scheduling |

### Why Rust

- Learning goal: build production Rust skills on a real project
- Good fit for streaming data with async/await (tokio)
- Cap'n Proto has solid Rust support
- Sets foundation for higher-performance markets (Kraken) later

### Why Go for Tooling

- Faster iteration for CLI (cobra ecosystem)
- Temporal SDK is mature in Go
- Already specified in design brief

## Data Flow

### Wire Formats

| Path | Format | Rationale |
|------|--------|-----------|
| Kalshi → Connector | JSON (WebSocket) | Exchange native format |
| Connector → NATS | Cap'n Proto | Compact, schema-enforced, learning goal |
| NATS → Archiver | Cap'n Proto | Consistent internal format |
| Gateway → Clients | JSON | Human/agent readable |
| Raw storage | JSONL (compressed) | Preserve original exchange data |
| Normalized storage | Cap'n Proto | Compact, typed, replayable |

### NATS Subjects

```
kalshi.raw.{event_type}           # Raw events from connector
kalshi.normalized.{event_type}    # Normalized events
kalshi.trade.{ticker}             # Per-symbol trade stream
kalshi.orderbook.{ticker}         # Per-symbol orderbook updates
```

JetStream consumers provide replay capability and persistence.

## Cap'n Proto Schema

```capnp
@0xabcdef1234567890;

struct Trade {
  timestamp @0 :UInt64;        # Unix nanos
  ticker @1 :Text;
  price @2 :Float64;
  size @3 :UInt32;
  side @4 :Side;
  tradeId @5 :Text;
}

enum Side {
  buy @0;
  sell @1;
}

struct OrderBookUpdate {
  timestamp @0 :UInt64;
  ticker @1 :Text;
  bids @2 :List(Level);
  asks @3 :List(Level);
}

struct Level {
  price @0 :Float64;
  size @1 :UInt32;
}

struct MarketStatus {
  timestamp @0 :UInt64;
  ticker @1 :Text;
  status @2 :Status;
}

enum Status {
  open @0;
  closed @1;
  halted @2;
}
```

## Security Master

PostgreSQL stores all market metadata. Essential for prediction markets where contracts expire.

### Schema

```sql
CREATE TABLE markets (
  id SERIAL PRIMARY KEY,
  ticker VARCHAR(64) UNIQUE NOT NULL,
  kalshi_id VARCHAR(128) NOT NULL,
  title TEXT NOT NULL,
  category VARCHAR(64),
  status VARCHAR(16) NOT NULL DEFAULT 'active',

  -- Contract details
  open_time TIMESTAMPTZ,
  close_time TIMESTAMPTZ,
  expiration_time TIMESTAMPTZ,
  settlement_time TIMESTAMPTZ,

  -- Settlement
  result VARCHAR(16),  -- 'yes', 'no', NULL if unsettled
  settled_at TIMESTAMPTZ,

  -- Metadata
  created_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW(),
  raw_metadata JSONB
);

CREATE TABLE market_history (
  id SERIAL PRIMARY KEY,
  market_id INTEGER REFERENCES markets(id),
  field_name VARCHAR(64) NOT NULL,
  old_value TEXT,
  new_value TEXT,
  changed_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_markets_ticker ON markets(ticker);
CREATE INDEX idx_markets_status ON markets(status);
CREATE INDEX idx_markets_expiration ON markets(expiration_time);
```

### Sync Job

Temporal workflow syncs markets from Kalshi API daily (and on startup):

1. Fetch all active markets from Kalshi
2. Upsert into PostgreSQL
3. Record changes in market_history
4. Publish change events to NATS for downstream consumers

## Daily Operations

### Teardown/Startup Cycle

The system tears down at end of day and starts fresh. This ensures:
- No accumulated state drift
- Clean recovery from any issues
- Forced validation of startup procedures

**Schedule (UTC):**
```
00:00 - Teardown begins
00:05 - All pods terminated
00:10 - Startup begins
00:15 - System healthy, streaming resumes
```

### Temporal Workflows

```go
// DailyOperations workflow
func DailyOperations(ctx workflow.Context, date time.Time) error {
    // 1. Sync security master from Kalshi
    err := workflow.ExecuteActivity(ctx, SyncSecurityMaster).Get(ctx, nil)
    if err != nil {
        return err
    }

    // 2. Start connector
    err = workflow.ExecuteActivity(ctx, StartConnector).Get(ctx, nil)
    if err != nil {
        return err
    }

    // 3. Start archiver
    err = workflow.ExecuteActivity(ctx, StartArchiver).Get(ctx, nil)
    if err != nil {
        return err
    }

    // 4. Start gateway
    err = workflow.ExecuteActivity(ctx, StartGateway).Get(ctx, nil)
    if err != nil {
        return err
    }

    // 5. Health check
    err = workflow.ExecuteActivity(ctx, HealthCheck).Get(ctx, nil)
    if err != nil {
        return err
    }

    return nil
}

// DailyTeardown workflow
func DailyTeardown(ctx workflow.Context) error {
    // 1. Stop accepting new connections
    err := workflow.ExecuteActivity(ctx, DrainGateway).Get(ctx, nil)
    if err != nil {
        return err
    }

    // 2. Flush archiver buffers
    err = workflow.ExecuteActivity(ctx, FlushArchiver).Get(ctx, nil)
    if err != nil {
        return err
    }

    // 3. Stop connector
    err = workflow.ExecuteActivity(ctx, StopConnector).Get(ctx, nil)
    if err != nil {
        return err
    }

    // 4. Final archive verification
    err = workflow.ExecuteActivity(ctx, VerifyArchive).Get(ctx, nil)
    if err != nil {
        return err
    }

    return nil
}
```

## Secrets Management

Sealed Secrets for GitOps-compatible secret storage. Secrets include:
- Kalshi API credentials
- NATS credentials
- PostgreSQL credentials
- S3 access keys

```yaml
# sealed-secrets/kalshi-creds.yaml
apiVersion: bitnami.com/v1alpha1
kind: SealedSecret
metadata:
  name: kalshi-creds
  namespace: ssmd
spec:
  encryptedData:
    api_key: AgBy8hCi...  # Encrypted
    api_secret: AgDk2Lx...  # Encrypted
```

Vaultwarden available for future migration when dynamic secrets needed.

## Storage Layout

### S3 Buckets

```
ssmd-raw/
  kalshi/
    2025/12/14/
      trades-00.jsonl.zst
      trades-01.jsonl.zst
      orderbook-00.jsonl.zst

ssmd-normalized/
  kalshi/
    v1/
      trade/
        2025/12/14/
          {ticker}/
            data.capnp.zst
      orderbook/
        2025/12/14/
          {ticker}/
            data.capnp.zst
```

### Raw Format

Compressed JSONL preserving original Kalshi messages:

```json
{"ts":1702540800000,"type":"trade","data":{"ticker":"INXD-25-B4000","price":0.45,"count":10}}
{"ts":1702540800100,"type":"orderbook","data":{"ticker":"INXD-25-B4000","yes_bid":0.44,"yes_ask":0.46}}
```

## Gateway API

### WebSocket

```
ws://gateway.ssmd.local/v1/stream?symbols=INXD-25-B4000,KXBTC-25DEC31

# Subscribe message
{"action": "subscribe", "symbols": ["INXD-25-B4000"]}

# Trade event
{"type": "trade", "ticker": "INXD-25-B4000", "price": 0.45, "size": 10, "side": "buy", "ts": 1702540800000}

# Orderbook snapshot
{"type": "orderbook", "ticker": "INXD-25-B4000", "bids": [[0.44, 100]], "asks": [[0.46, 150]], "ts": 1702540800000}
```

### REST

```
GET /v1/markets                    # List all markets
GET /v1/markets/{ticker}           # Market details
GET /v1/markets/{ticker}/trades    # Recent trades
GET /v1/health                     # System health
```

## CLI

```bash
# Environment management
ssmd env create kalshi-prod --from environments/kalshi.yaml
ssmd env validate environments/kalshi.yaml
ssmd env apply environments/kalshi.yaml

# Market operations
ssmd market list
ssmd market sync                   # Force secmaster sync
ssmd market show INXD-25-B4000

# Operations
ssmd ops start                     # Trigger startup workflow
ssmd ops stop                      # Trigger teardown workflow
ssmd ops status                    # Current system state

# Data operations
ssmd data replay --date 2025-12-14 --symbol INXD-25-B4000
ssmd data export --date 2025-12-14 --format parquet
```

## Environment Definition

```yaml
# environments/kalshi.yaml
apiVersion: ssmd/v1
kind: Environment
metadata:
  name: kalshi-prod

spec:
  feed: kalshi

  schedule:
    timezone: UTC
    startup: "00:10"
    teardown: "00:00"

  symbols:
    # Can be explicit list or use selectors
    sync: true  # Auto-sync from Kalshi API
    filters:
      - category: "financials"
      - category: "crypto"

  transport:
    type: nats
    url: nats://nats.ssmd.local:4222
    stream: ssmd-kalshi

  storage:
    raw:
      bucket: ssmd-raw
      endpoint: s3.homelab.local
    normalized:
      bucket: ssmd-normalized
      endpoint: s3.homelab.local

  secrets:
    kalshi: sealed-secret/kalshi-creds
    nats: sealed-secret/nats-creds
    postgres: sealed-secret/postgres-creds
    s3: sealed-secret/s3-creds
```

## Observability

### Metrics (Prometheus)

```
# Connector
ssmd_connector_messages_received_total{feed="kalshi",type="trade"}
ssmd_connector_messages_published_total{feed="kalshi"}
ssmd_connector_lag_seconds{feed="kalshi"}
ssmd_connector_errors_total{feed="kalshi",error_type="parse"}

# Gateway
ssmd_gateway_clients_connected
ssmd_gateway_messages_sent_total{type="trade"}
ssmd_gateway_subscriptions_active{symbol="INXD-25-B4000"}

# Archiver
ssmd_archiver_bytes_written_total{bucket="raw"}
ssmd_archiver_files_written_total{bucket="normalized"}
```

### Alerts

```yaml
# Critical: No data flowing
- alert: ConnectorNoData
  expr: rate(ssmd_connector_messages_received_total[5m]) == 0
  for: 2m
  labels:
    severity: critical

# Warning: High lag
- alert: ConnectorHighLag
  expr: ssmd_connector_lag_seconds > 5
  for: 1m
  labels:
    severity: warning
```

### Logs

Structured JSON to stdout, collected with Loki:

```json
{"level":"info","ts":"2025-12-14T00:10:00Z","component":"connector","msg":"connected to kalshi","symbols":42}
{"level":"info","ts":"2025-12-14T00:10:01Z","component":"connector","msg":"trade","ticker":"INXD-25-B4000","price":0.45}
```

## Implementation Phases

### Phase 1: Foundation (Week 1)

- [ ] Rust project setup (cargo workspace)
- [ ] Cap'n Proto schema definition
- [ ] Kalshi WebSocket client (tokio + tungstenite)
- [ ] Basic NATS publisher
- [ ] PostgreSQL secmaster schema

**Deliverable:** Connector prints Kalshi trades to stdout.

### Phase 2: Streaming (Week 2)

- [ ] Connector publishes to NATS (Cap'n Proto)
- [ ] Gateway subscribes to NATS
- [ ] Gateway serves WebSocket (JSON)
- [ ] Basic CLI (Go) for operations

**Deliverable:** Can connect via WebSocket and see live trades.

### Phase 3: Persistence (Week 3)

- [ ] Raw archiver (JSONL to S3)
- [ ] Normalized archiver (Cap'n Proto to S3)
- [ ] Secmaster sync from Kalshi API
- [ ] Temporal workflows for startup/teardown

**Deliverable:** Data persists across restarts.

### Phase 4: Operations (Week 4)

- [ ] Sealed Secrets integration
- [ ] Full CLI implementation
- [ ] Prometheus metrics
- [ ] ArgoCD manifests
- [ ] Documentation

**Deliverable:** Production-ready deployment.

## Dependencies

| Dependency | Version | Purpose |
|------------|---------|---------|
| tokio | 1.x | Async runtime |
| tungstenite | 0.21 | WebSocket client |
| capnp | 0.18 | Cap'n Proto |
| async-nats | 0.33 | NATS client |
| sqlx | 0.7 | PostgreSQL |
| serde | 1.x | JSON serialization |
| tracing | 0.1 | Structured logging |

## Open Questions

1. **Kalshi rate limits** - Need to verify API limits for market sync
2. **Orderbook depth** - Full book or top N levels?
3. **Historical backfill** - Does Kalshi provide historical data API?
4. **Client auth** - API keys sufficient or need more?

## Future Work (Post-Milestone)

- Polymarket connector
- Kraken connector (libechidna/C++ integration)
- TUI admin interface
- Agent feedback API for data quality issues
- Lua transforms for custom client formats
- Multi-tenant support
- Vaultwarden integration for dynamic secrets

---

*Design created: 2025-12-14*
