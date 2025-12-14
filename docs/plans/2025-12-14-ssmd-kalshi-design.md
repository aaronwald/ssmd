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

## Metadata System

Metadata is the foundation for operator safety. The system is self-describing: every feed, symbol, schema version, and data file is tracked. Operators cannot misconfigure what they can query.

### Design Principles

1. **Query before act** - CLI validates against metadata before any operation
2. **No implicit state** - All configuration is explicit and versioned
3. **Fail fast** - Invalid references fail at config time, not runtime
4. **Time-travel** - All metadata is temporal; query state as-of any date

### Metadata Domains

```
┌─────────────────────────────────────────────────────────────────┐
│                      METADATA REGISTRY                          │
├─────────────────┬─────────────────┬─────────────────────────────┤
│  Feed Registry  │  Data Inventory │  Schema Registry            │
│  - Exchanges    │  - Coverage     │  - Versions                 │
│  - Protocols    │  - Gaps         │  - Migrations               │
│  - Credentials  │  - Quality      │  - Compatibility            │
├─────────────────┴─────────────────┴─────────────────────────────┤
│                    System Configuration                          │
│  - Environments  - Deployments  - Audit Log                     │
└─────────────────────────────────────────────────────────────────┘
```

### 1. Feed Registry

Defines what data sources exist and how to connect to them.

```sql
CREATE TABLE feeds (
  id SERIAL PRIMARY KEY,
  name VARCHAR(64) UNIQUE NOT NULL,      -- 'kalshi', 'polymarket', 'kraken'
  display_name VARCHAR(128),
  feed_type VARCHAR(32) NOT NULL,        -- 'websocket', 'rest', 'multicast'
  status VARCHAR(16) DEFAULT 'active',   -- 'active', 'deprecated', 'disabled'
  created_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE feed_versions (
  id SERIAL PRIMARY KEY,
  feed_id INTEGER REFERENCES feeds(id),
  version VARCHAR(32) NOT NULL,          -- 'v1', 'v2'
  effective_from DATE NOT NULL,
  effective_to DATE,                     -- NULL = current

  -- Connection details
  protocol VARCHAR(32) NOT NULL,         -- 'wss', 'https', 'multicast'
  endpoint_template TEXT NOT NULL,       -- 'wss://api.kalshi.com/trade-api/ws/v2'
  auth_method VARCHAR(32),               -- 'api_key', 'oauth', 'mtls'
  secret_ref VARCHAR(128),               -- 'sealed-secret/kalshi-creds'

  -- Capabilities
  supports_orderbook BOOLEAN DEFAULT false,
  supports_trades BOOLEAN DEFAULT true,
  supports_historical BOOLEAN DEFAULT false,
  max_symbols_per_connection INTEGER,
  rate_limit_per_second INTEGER,

  -- Parser configuration
  parser_config JSONB,                   -- Feed-specific parsing rules

  UNIQUE(feed_id, effective_from)
);

CREATE TABLE feed_calendars (
  id SERIAL PRIMARY KEY,
  feed_id INTEGER REFERENCES feeds(id),
  effective_from DATE NOT NULL,
  effective_to DATE,

  -- Trading hours (NULL = 24/7)
  timezone VARCHAR(64),
  open_time TIME,
  close_time TIME,

  -- Holiday calendar reference
  holiday_calendar VARCHAR(64),          -- 'us_equity', 'crypto_247', 'custom'

  UNIQUE(feed_id, effective_from)
);
```

**Example: Adding a new feed**

```bash
# CLI validates feed definition before insert
ssmd feed create polymarket \
  --type websocket \
  --protocol wss \
  --endpoint 'wss://ws-subscriptions-clob.polymarket.com/ws/market' \
  --auth-method api_key \
  --secret sealed-secret/polymarket-creds

# Show feed configuration as-of a date
ssmd feed show kalshi --as-of 2025-12-01

# List all active feeds
ssmd feed list --status active
```

### 2. Data Inventory

Tracks what data exists, where it lives, and its quality status.

```sql
CREATE TABLE data_inventory (
  id SERIAL PRIMARY KEY,
  feed_id INTEGER REFERENCES feeds(id),
  data_type VARCHAR(32) NOT NULL,        -- 'raw', 'normalized'
  date DATE NOT NULL,

  -- Location
  storage_path TEXT NOT NULL,            -- 's3://ssmd-raw/kalshi/2025/12/14/'
  schema_version VARCHAR(32),            -- 'v1' (for normalized)

  -- Coverage
  symbol_count INTEGER,
  record_count BIGINT,
  byte_size BIGINT,
  first_timestamp TIMESTAMPTZ,
  last_timestamp TIMESTAMPTZ,

  -- Quality
  status VARCHAR(16) NOT NULL,           -- 'complete', 'partial', 'failed', 'processing'
  gap_count INTEGER DEFAULT 0,
  quality_score DECIMAL(3,2),            -- 0.00 to 1.00

  -- Provenance
  connector_version VARCHAR(32),
  processor_version VARCHAR(32),
  processed_at TIMESTAMPTZ,

  UNIQUE(feed_id, data_type, date, schema_version)
);

CREATE TABLE data_gaps (
  id SERIAL PRIMARY KEY,
  inventory_id INTEGER REFERENCES data_inventory(id),
  gap_start TIMESTAMPTZ NOT NULL,
  gap_end TIMESTAMPTZ NOT NULL,
  gap_type VARCHAR(32),                  -- 'connection_lost', 'rate_limited', 'exchange_outage'
  resolved BOOLEAN DEFAULT false,
  resolved_at TIMESTAMPTZ,
  notes TEXT
);

CREATE TABLE data_quality_issues (
  id SERIAL PRIMARY KEY,
  inventory_id INTEGER REFERENCES data_inventory(id),
  issue_type VARCHAR(32) NOT NULL,       -- 'duplicate', 'out_of_order', 'missing_field', 'parse_error'
  severity VARCHAR(16) NOT NULL,         -- 'error', 'warning', 'info'
  count INTEGER DEFAULT 1,
  sample_data JSONB,
  detected_at TIMESTAMPTZ DEFAULT NOW()
);
```

**Example: Querying data inventory**

```bash
# What data do we have for a date range?
ssmd data inventory --feed kalshi --from 2025-12-01 --to 2025-12-14

# Show gaps for a specific date
ssmd data gaps --feed kalshi --date 2025-12-14

# Data quality report
ssmd data quality --feed kalshi --date 2025-12-14

# Find dates with incomplete data
ssmd data inventory --feed kalshi --status partial
```

**Inventory output example:**

```
Feed: kalshi
Date Range: 2025-12-01 to 2025-12-14

Date        Raw     Normalized  Status     Quality  Gaps
2025-12-01  523MB   312MB       complete   0.99     0
2025-12-02  518MB   308MB       complete   1.00     0
2025-12-03  531MB   319MB       partial    0.95     2
...
2025-12-14  -       -           processing -        -
```

### 3. Schema Registry

Tracks schema versions for normalized data. Ensures you can always reprocess with the correct schema.

```sql
CREATE TABLE schema_versions (
  id SERIAL PRIMARY KEY,
  name VARCHAR(64) NOT NULL,             -- 'trade', 'orderbook', 'market_status'
  version VARCHAR(32) NOT NULL,

  -- Schema definition
  format VARCHAR(32) NOT NULL,           -- 'capnp', 'protobuf', 'json_schema'
  schema_definition TEXT NOT NULL,       -- Actual schema content
  schema_hash VARCHAR(64) NOT NULL,      -- SHA256 for integrity

  -- Lifecycle
  status VARCHAR(16) DEFAULT 'active',   -- 'draft', 'active', 'deprecated'
  effective_from DATE NOT NULL,
  deprecated_at DATE,

  -- Compatibility
  compatible_with JSONB,                 -- ['v1', 'v2'] - can read these versions
  breaking_changes TEXT,                 -- Description of what changed

  created_at TIMESTAMPTZ DEFAULT NOW(),

  UNIQUE(name, version)
);

CREATE TABLE schema_migrations (
  id SERIAL PRIMARY KEY,
  from_version_id INTEGER REFERENCES schema_versions(id),
  to_version_id INTEGER REFERENCES schema_versions(id),

  -- Migration script
  migration_type VARCHAR(32),            -- 'automatic', 'manual', 'reprocess'
  migration_script TEXT,

  -- Execution tracking
  executed_at TIMESTAMPTZ,
  executed_by VARCHAR(64),
  status VARCHAR(16),                    -- 'pending', 'running', 'completed', 'failed'

  UNIQUE(from_version_id, to_version_id)
);
```

**Example: Schema operations**

```bash
# List schema versions
ssmd schema list

# Show schema for a specific version
ssmd schema show trade --version v1

# Check compatibility
ssmd schema compat trade --from v1 --to v2

# What schema was used on a date?
ssmd schema used --feed kalshi --date 2025-12-14
```

### 4. System Configuration

The environment definition itself is versioned metadata.

```sql
CREATE TABLE environments (
  id SERIAL PRIMARY KEY,
  name VARCHAR(64) UNIQUE NOT NULL,      -- 'kalshi-prod', 'kalshi-dev'
  description TEXT,
  status VARCHAR(16) DEFAULT 'active',
  created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE environment_versions (
  id SERIAL PRIMARY KEY,
  environment_id INTEGER REFERENCES environments(id),
  version INTEGER NOT NULL,

  -- Configuration snapshot
  config_yaml TEXT NOT NULL,
  config_hash VARCHAR(64) NOT NULL,

  -- Deployment tracking
  deployed_at TIMESTAMPTZ,
  deployed_by VARCHAR(64),
  git_commit VARCHAR(40),

  -- Validity
  valid_from TIMESTAMPTZ,
  valid_to TIMESTAMPTZ,                  -- NULL = current

  UNIQUE(environment_id, version)
);

CREATE TABLE deployment_log (
  id SERIAL PRIMARY KEY,
  environment_version_id INTEGER REFERENCES environment_versions(id),
  action VARCHAR(32) NOT NULL,           -- 'deploy', 'teardown', 'rollback'
  status VARCHAR(16) NOT NULL,           -- 'started', 'completed', 'failed'
  started_at TIMESTAMPTZ DEFAULT NOW(),
  completed_at TIMESTAMPTZ,
  error_message TEXT,

  -- What triggered this
  trigger VARCHAR(32),                   -- 'scheduled', 'manual', 'gitops'
  triggered_by VARCHAR(64)
);
```

**Example: Environment operations**

```bash
# Show current environment configuration
ssmd env show kalshi-prod

# Show environment as it was deployed on a date
ssmd env show kalshi-prod --as-of 2025-12-10

# Diff two versions
ssmd env diff kalshi-prod --v1 3 --v2 4

# Deployment history
ssmd env history kalshi-prod

# Validate before deploy (queries all metadata)
ssmd env validate environments/kalshi.yaml
```

### Validation: Removing Operator Error

Every CLI command validates against metadata before acting:

```bash
$ ssmd connector start --feed polymarket --symbols BTC-WINNER

Error: Validation failed
  - Feed 'polymarket' not found in feed registry
  - Did you mean 'kalshi'?

Available feeds:
  kalshi (active) - Prediction market

$ ssmd data replay --feed kalshi --date 2025-12-20

Error: Validation failed
  - No data inventory for kalshi on 2025-12-20
  - Latest available: 2025-12-14

$ ssmd env apply environments/kalshi.yaml

Validating environment...
  ✓ Feed 'kalshi' exists and is active
  ✓ Schema 'trade:v1' exists and is active
  ✓ Secret 'sealed-secret/kalshi-creds' exists
  ✓ Storage bucket 'ssmd-raw' accessible
  ✓ NATS stream 'ssmd-kalshi' exists

Ready to deploy. Proceed? [y/N]
```

### Metadata API

All metadata is queryable via REST for agents and tooling:

```
GET /v1/meta/feeds                       # List feeds
GET /v1/meta/feeds/{name}                # Feed details
GET /v1/meta/feeds/{name}/calendar       # Trading calendar

GET /v1/meta/inventory                   # Data inventory
GET /v1/meta/inventory/{feed}/{date}     # Specific date
GET /v1/meta/gaps/{feed}                 # Data gaps

GET /v1/meta/schemas                     # Schema versions
GET /v1/meta/schemas/{name}/{version}    # Specific schema

GET /v1/meta/environments                # Environments
GET /v1/meta/environments/{name}/history # Deployment history
```

### Temporal Queries (As-Of)

All metadata supports temporal queries:

```sql
-- What was the Kalshi endpoint on Dec 1st?
SELECT * FROM feed_versions
WHERE feed_id = (SELECT id FROM feeds WHERE name = 'kalshi')
  AND effective_from <= '2025-12-01'
  AND (effective_to IS NULL OR effective_to > '2025-12-01');

-- What schema version was used to process Dec 14th data?
SELECT sv.* FROM schema_versions sv
JOIN data_inventory di ON di.schema_version = sv.version
WHERE di.feed_id = (SELECT id FROM feeds WHERE name = 'kalshi')
  AND di.date = '2025-12-14'
  AND di.data_type = 'normalized';
```

This enables reproducible backtesting: replay Dec 1st data with the exact configuration and schema that was active on Dec 1st.

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

Part of the Metadata System. Stores market/instrument metadata for each feed. Essential for prediction markets where contracts expire.

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

### Phase 1: Metadata Foundation (Week 1)

Metadata first - the system must know what it's managing before managing it.

**Database schema:**
- [ ] PostgreSQL metadata schema (feeds, feed_versions, feed_calendars)
- [ ] Data inventory schema (data_inventory, data_gaps, data_quality_issues)
- [ ] Schema registry tables (schema_versions, schema_migrations)
- [ ] Environment/deployment tables (environments, environment_versions, deployment_log)
- [ ] Security master tables (markets, market_history)

**CLI foundation (Go):**
- [ ] `ssmd feed create/list/show` - feed registry management
- [ ] `ssmd schema register/list/show` - schema registry
- [ ] `ssmd env validate` - environment validation against metadata
- [ ] Validation framework: all commands query metadata before acting

**Bootstrap data:**
- [ ] Register Kalshi feed in feed_versions
- [ ] Register Cap'n Proto schemas (trade, orderbook, market_status)
- [ ] Create initial environment definition

**Deliverable:** Can run `ssmd feed list` and see Kalshi. `ssmd env validate` checks all references.

### Phase 2: Connector + Streaming (Week 2)

**Rust connector:**
- [ ] Rust project setup (cargo workspace)
- [ ] Cap'n Proto schema definition (.capnp files)
- [ ] Kalshi WebSocket client (tokio + tungstenite)
- [ ] Connector reads feed config from metadata DB
- [ ] Basic NATS publisher (Cap'n Proto)

**Gateway:**
- [ ] Gateway subscribes to NATS
- [ ] Gateway serves WebSocket (JSON translation)
- [ ] Metadata API endpoints (`/v1/meta/feeds`, `/v1/meta/schemas`)

**Deliverable:** Live trades visible via WebSocket. Metadata queryable via REST.

### Phase 3: Persistence + Inventory (Week 3)

**Archival:**
- [ ] Raw archiver (JSONL to S3)
- [ ] Normalized archiver (Cap'n Proto to S3)
- [ ] Archiver writes to data_inventory on completion
- [ ] Gap detection: archiver records disconnections to data_gaps

**Security master sync:**
- [ ] Temporal workflow: sync markets from Kalshi API
- [ ] Record changes in market_history
- [ ] Publish change events to NATS

**Data inventory CLI:**
- [ ] `ssmd data inventory --feed kalshi` - show what data exists
- [ ] `ssmd data gaps --feed kalshi --date DATE` - show gaps
- [ ] `ssmd data quality --feed kalshi --date DATE` - quality report

**Deliverable:** Data persists. Can query `ssmd data inventory` to see coverage.

### Phase 4: Operations + Scheduling (Week 4)

**Temporal workflows:**
- [ ] Daily startup workflow (sync metadata → start connector → start archiver → start gateway)
- [ ] Daily teardown workflow (drain → flush → stop → verify)
- [ ] Workflow writes to deployment_log

**Secrets + deployment:**
- [ ] Sealed Secrets integration
- [ ] ArgoCD manifests
- [ ] Environment versioning: `ssmd env apply` creates environment_version record

**Observability:**
- [ ] Prometheus metrics
- [ ] Metadata-aware alerts (e.g., gap detected → alert)

**CLI completion:**
- [ ] `ssmd env apply/diff/history`
- [ ] `ssmd ops start/stop/status`
- [ ] `ssmd data replay --date DATE`

**Deliverable:** Production-ready. Daily cycle automated. Full audit trail in metadata.

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
