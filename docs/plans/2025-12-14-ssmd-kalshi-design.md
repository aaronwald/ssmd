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
- Complex routing or tickerplant functionality

## First Milestone Scope

Kalshi only. Polymarket and Kraken follow after the foundation is proven.

## Key Management

Keys are first-class citizens. Every environment starts with key definitions. The system makes secret management easy - no manual kubectl or kubeseal operations required.

### Design Principles

1. **Keys first** - Environment definitions start with keys, not infrastructure
2. **Declarative** - Keys defined in YAML, CLI handles encryption/storage
3. **Validated** - All key references validated before deployment
4. **Rotatable** - Keys can be rotated without redeployment
5. **Audited** - All key access and changes logged

### Key Types

| Type | Purpose | Examples |
|------|---------|----------|
| `api_key` | Exchange API credentials | Kalshi API key/secret |
| `database` | Database connections | PostgreSQL credentials |
| `transport` | Message broker auth | NATS credentials |
| `storage` | Object storage access | S3 access key/secret |
| `tls` | Certificates | mTLS certs, CA bundles |
| `webhook` | Callback authentication | Agent webhook secrets |

### Environment Keys Definition

Keys are the **first section** in every environment file:

```yaml
# environments/kalshi.yaml
apiVersion: ssmd/v1
kind: Environment
metadata:
  name: kalshi-prod

spec:
  # KEYS FIRST - before anything else
  keys:
    # Exchange credentials
    kalshi:
      type: api_key
      description: "Kalshi trading API"
      required: true
      fields:
        - api_key
        - api_secret
      rotation_days: 90

    # Infrastructure credentials
    postgres:
      type: database
      description: "ssmd metadata database"
      required: true
      fields:
        - host
        - port
        - database
        - username
        - password

    nats:
      type: transport
      description: "NATS messaging"
      required: true
      fields:
        - url
        - username
        - password
        - tls_cert      # optional
        - tls_key       # optional

    storage:
      type: storage
      description: "Object storage (when Garage ready)"
      required: false   # Optional until Brooklyn NAS
      fields:
        - endpoint
        - access_key
        - secret_key
        - region

  # Then feed, middleware, etc...
  feed: kalshi
  # ...
```

### Key Storage Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         ssmd CLI                                     │
│  ssmd key set kalshi --api-key xxx --api-secret yyy                │
└───────────────────────────────┬─────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Key Manager                                     │
│  - Validates key fields against environment spec                    │
│  - Encrypts with Sealed Secrets public key                         │
│  - Stores encrypted values                                          │
└───────────────────────────────┬─────────────────────────────────────┘
                                │
              ┌─────────────────┼─────────────────┐
              ▼                 ▼                 ▼
       ┌───────────┐     ┌───────────┐     ┌───────────┐
       │  Sealed   │     │ Metadata  │     │   Audit   │
       │  Secret   │     │    DB     │     │    Log    │
       │  (K8s)    │     │  (refs)   │     │           │
       └───────────┘     └───────────┘     └───────────┘
```

### CLI Commands

```bash
# Initialize keys for a new environment (interactive)
ssmd key init kalshi-prod
# Prompts for each required key defined in environment spec

# Set individual key
ssmd key set kalshi-prod kalshi --api-key xxx --api-secret yyy

# Set from file (for complex keys like TLS certs)
ssmd key set kalshi-prod nats --from-file tls_cert=./cert.pem --from-file tls_key=./key.pem

# Set from environment variables
export KALSHI_API_KEY=xxx
export KALSHI_API_SECRET=yyy
ssmd key set kalshi-prod kalshi --from-env KALSHI_API_KEY,KALSHI_API_SECRET

# List keys (shows metadata, not values)
ssmd key list kalshi-prod
# NAME      TYPE      REQUIRED  STATUS    LAST_ROTATED  EXPIRES
# kalshi    api_key   yes       set       2025-12-14    2026-03-14
# postgres  database  yes       set       2025-12-14    -
# nats      transport yes       set       2025-12-14    -
# storage   storage   no        not_set   -             -

# Verify all required keys are set
ssmd key verify kalshi-prod
# ✓ kalshi: set (expires in 90 days)
# ✓ postgres: set
# ✓ nats: set
# ○ storage: not set (optional)
# All required keys present.

# Show key metadata (not the value)
ssmd key show kalshi-prod kalshi
# Name: kalshi
# Type: api_key
# Status: set
# Fields: api_key, api_secret
# Last Rotated: 2025-12-14T10:30:00Z
# Rotation Policy: 90 days
# Expires: 2026-03-14T10:30:00Z
# Sealed Secret: ssmd/kalshi-prod-kalshi

# Rotate a key
ssmd key rotate kalshi-prod kalshi --api-key NEW_KEY --api-secret NEW_SECRET

# Delete a key
ssmd key delete kalshi-prod storage

# Export key references (for GitOps, no actual secrets)
ssmd key export kalshi-prod > keys-manifest.yaml
```

### Key Validation

Before any deployment, all key references are validated:

```bash
$ ssmd env apply kalshi.yaml

Validating environment...
  ✓ Keys section present
  ✓ Key 'kalshi' defined (api_key, required)
  ✓ Key 'postgres' defined (database, required)
  ✓ Key 'nats' defined (transport, required)
  ○ Key 'storage' defined (storage, optional)

Checking key status...
  ✓ Key 'kalshi' is set
  ✓ Key 'postgres' is set
  ✗ Key 'nats' is NOT SET

Error: Required key 'nats' is not set.
Run: ssmd key set kalshi-prod nats --url xxx --username yyy --password zzz
```

### Key References in Config

Components reference keys by name, never raw values:

```yaml
# Connector config references key by name
connector:
  feed: kalshi
  credentials: $key:kalshi    # Resolved at runtime

# Middleware references keys
middleware:
  transport:
    type: nats
    credentials: $key:nats    # Resolved at runtime
  storage:
    type: s3
    credentials: $key:storage # Optional, only if set
```

### Metadata Schema

```sql
CREATE TABLE keys (
  id SERIAL PRIMARY KEY,
  environment_id INTEGER REFERENCES environments(id),
  name VARCHAR(64) NOT NULL,
  key_type VARCHAR(32) NOT NULL,       -- 'api_key', 'database', etc.
  description TEXT,
  required BOOLEAN DEFAULT true,
  fields JSONB NOT NULL,               -- ['api_key', 'api_secret']
  rotation_days INTEGER,

  -- Status
  status VARCHAR(16) DEFAULT 'not_set', -- 'not_set', 'set', 'expired'
  sealed_secret_ref VARCHAR(128),       -- 'ssmd/kalshi-prod-kalshi'

  -- Audit
  last_rotated TIMESTAMPTZ,
  expires_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW(),

  UNIQUE(environment_id, name)
);

CREATE TABLE key_audit_log (
  id SERIAL PRIMARY KEY,
  key_id INTEGER REFERENCES keys(id),
  action VARCHAR(32) NOT NULL,         -- 'created', 'rotated', 'accessed', 'deleted'
  actor VARCHAR(64),                   -- 'cli:user@host', 'system:connector'
  details JSONB,
  created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_keys_environment ON keys(environment_id);
CREATE INDEX idx_key_audit_key_id ON key_audit_log(key_id);
CREATE INDEX idx_key_audit_created ON key_audit_log(created_at);
```

### Runtime Key Resolution

Components resolve keys at startup:

```rust
pub struct KeyResolver {
    k8s_client: kube::Client,
    cache: HashMap<String, KeyValue>,
}

impl KeyResolver {
    pub async fn resolve(&self, key_ref: &str) -> Result<KeyValue, KeyError> {
        // Parse reference: "$key:kalshi" -> "kalshi"
        let key_name = key_ref.strip_prefix("$key:").ok_or(KeyError::InvalidRef)?;

        // Check cache
        if let Some(cached) = self.cache.get(key_name) {
            return Ok(cached.clone());
        }

        // Load from Sealed Secret
        let secret_name = format!("{}-{}", self.environment, key_name);
        let secret = self.k8s_client
            .get::<Secret>(&secret_name, &self.namespace)
            .await?;

        let value = KeyValue::from_secret(&secret)?;
        self.cache.insert(key_name.to_string(), value.clone());

        // Log access
        self.audit_log(key_name, "accessed").await;

        Ok(value)
    }
}
```

### Key Expiration Alerts

```yaml
# Prometheus alert for expiring keys
- alert: KeyExpiringSoon
  expr: ssmd_key_expires_in_days < 14
  for: 1h
  labels:
    severity: warning
  annotations:
    summary: "Key {{ $labels.key_name }} expires in {{ $value }} days"
    runbook: "Run: ssmd key rotate {{ $labels.environment }} {{ $labels.key_name }}"

- alert: KeyExpired
  expr: ssmd_key_expires_in_days < 0
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "Key {{ $labels.key_name }} has EXPIRED"
```

### Workflow: New Environment Setup

```bash
# 1. Create environment file with key definitions
cat > environments/kalshi-prod.yaml << 'EOF'
apiVersion: ssmd/v1
kind: Environment
metadata:
  name: kalshi-prod
spec:
  keys:
    kalshi:
      type: api_key
      required: true
      fields: [api_key, api_secret]
      rotation_days: 90
    postgres:
      type: database
      required: true
      fields: [host, port, database, username, password]
    nats:
      type: transport
      required: true
      fields: [url, username, password]
  feed: kalshi
  # ...
EOF

# 2. Initialize keys (interactive prompts)
ssmd key init kalshi-prod

# 3. Verify all keys set
ssmd key verify kalshi-prod

# 4. Deploy environment (keys validated automatically)
ssmd env apply environments/kalshi-prod.yaml
```

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

## Middleware Abstractions

All infrastructure dependencies are behind traits. Implementations are selected at deployment time via environment configuration - no code changes required to swap backends.

### Design Principles

1. **Trait-based** - Rust traits define the contract, implementations are pluggable
2. **Config-driven** - Environment YAML selects which implementation to use
3. **Runtime resolution** - Factory functions create the right implementation at startup
4. **Test-friendly** - In-memory implementations for testing without infrastructure

### Abstraction Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                        APPLICATION LAYER                             │
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

### 1. Transport Trait

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
| `AeronTransport` | `aeron-rs` | Low-latency, reliable multicast |
| `ChronicleTransport` | FFI | On-prem, shared memory |
| `InMemoryTransport` | built-in | Testing |

### 2. Storage Trait

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

### 3. Cache Trait

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

### 4. Journal Trait

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

### Factory Pattern

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

### Environment Configuration

Middleware selection in environment YAML:

```yaml
# environments/kalshi.yaml
apiVersion: ssmd/v1
kind: Environment
metadata:
  name: kalshi-prod

spec:
  feed: kalshi

  # Middleware configuration
  middleware:
    transport:
      type: nats                           # nats | aeron | chronicle | memory
      url: nats://nats.ssmd.local:4222
      # NATS-specific
      jetstream:
        enabled: true
        domain: ssmd
      # Aeron-specific (when type: aeron)
      # aeron:
      #   media_driver: /dev/shm/aeron
      #   channel: aeron:udp?endpoint=224.0.1.1:40456

    storage:
      type: s3                             # s3 | local | memory
      endpoint: http://garage.ssmd.local:3900
      region: garage
      buckets:
        raw: ssmd-raw
        normalized: ssmd-normalized
      # Local-specific (when type: local)
      # path: /var/lib/ssmd/storage

    cache:
      type: redis                          # redis | memory | none
      url: redis://redis.ssmd.local:6379
      max_connections: 10
      # Memory-specific (when type: memory)
      # max_size: 100MB

    journal:
      type: nats                           # nats | chronicle | file | memory
      url: nats://nats.ssmd.local:4222
      topics:
        secmaster: ssmd.journal.secmaster
        audit: ssmd.journal.audit
        deployments: ssmd.journal.deployments
      # File-specific (when type: file)
      # path: /var/lib/ssmd/journal
```

### Testing Configuration

In-memory implementations for tests:

```yaml
# environments/test.yaml
apiVersion: ssmd/v1
kind: Environment
metadata:
  name: test

spec:
  feed: kalshi-mock

  middleware:
    transport:
      type: memory
    storage:
      type: memory
    cache:
      type: memory
    journal:
      type: memory
```

### Middleware in Metadata Registry

The feed registry references which middleware is available:

```sql
-- Add to feed_versions table
ALTER TABLE feed_versions ADD COLUMN required_transport VARCHAR(32)[];
-- e.g., ['nats', 'aeron'] - feed works with these transports

-- Validation: environment's transport must be in feed's required_transport
```

CLI validates middleware compatibility:

```bash
$ ssmd env validate environments/kalshi.yaml

Validating environment...
  ✓ Feed 'kalshi' exists and is active
  ✓ Transport 'nats' is compatible with feed 'kalshi'
  ✓ Storage endpoint 'http://garage.ssmd.local:3900' is reachable
  ✓ Cache 'redis://redis.ssmd.local:6379' is reachable
  ✓ Journal topic 'ssmd.journal.secmaster' exists

Ready to deploy. Proceed? [y/N]
```

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

## Error Handling Strategy

Errors are categorized, handled consistently, and surfaced appropriately. The system fails fast on configuration errors and recovers gracefully from transient failures.

### Error Categories

| Category | Examples | Response |
|----------|----------|----------|
| **Configuration** | Invalid YAML, missing secret, unknown feed | Fail fast at startup, don't retry |
| **Transient** | Network timeout, rate limit, connection lost | Retry with backoff, then escalate |
| **Data Quality** | Parse error, unexpected schema, missing field | Log, record in inventory, continue |
| **Fatal** | Out of memory, disk full, auth revoked | Shutdown gracefully, alert |

### Retry Policy

Transient errors use exponential backoff with jitter:

```rust
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub jitter: f64,  // 0.0 to 1.0
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
            jitter: 0.1,
        }
    }
}

pub async fn retry_with_policy<F, T, E>(
    policy: &RetryPolicy,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Future<Output = Result<T, E>>,
    E: IsTransient,
{
    let mut attempt = 0;
    let mut delay = policy.initial_delay;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if e.is_transient() && attempt < policy.max_attempts => {
                attempt += 1;
                let jittered = add_jitter(delay, policy.jitter);
                tokio::time::sleep(jittered).await;
                delay = (delay.mul_f64(policy.multiplier)).min(policy.max_delay);
            }
            Err(e) => return Err(e),
        }
    }
}
```

### Dead Letter Queue

Messages that fail after all retries go to a dead letter queue for inspection:

```rust
pub struct DeadLetter {
    pub original_subject: String,
    pub payload: Bytes,
    pub error: String,
    pub attempts: u32,
    pub first_attempt: u64,
    pub last_attempt: u64,
    pub component: String,  // "connector", "archiver", etc.
}
```

Dead letters are:
1. Published to `ssmd.dlq.{component}` NATS subject
2. Recorded in `data_quality_issues` table
3. Visible via `ssmd dlq list` and TUI

```bash
# View dead letters
ssmd dlq list --component connector --since 1h

# Replay a dead letter (after fixing the issue)
ssmd dlq replay --id <dlq-id>

# Purge old dead letters
ssmd dlq purge --older-than 7d
```

### Circuit Breaker

Prevents cascade failures when downstream is unhealthy:

```rust
pub struct CircuitBreaker {
    state: AtomicU8,  // Closed=0, Open=1, HalfOpen=2
    failure_count: AtomicU32,
    success_count: AtomicU32,
    last_failure: AtomicU64,

    // Configuration
    failure_threshold: u32,      // Open after N failures
    success_threshold: u32,      // Close after N successes in half-open
    timeout: Duration,           // Time before half-open
}

impl CircuitBreaker {
    pub async fn call<F, T, E>(&self, operation: F) -> Result<T, CircuitError<E>>
    where
        F: Future<Output = Result<T, E>>,
    {
        match self.state() {
            State::Open => {
                if self.should_try_half_open() {
                    self.set_state(State::HalfOpen);
                } else {
                    return Err(CircuitError::Open);
                }
            }
            _ => {}
        }

        match operation.await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure();
                Err(CircuitError::Failed(e))
            }
        }
    }
}
```

Circuit breakers wrap:
- Exchange WebSocket connections
- NATS publish operations
- S3 storage operations
- PostgreSQL queries

### Error Propagation

Errors include context for debugging:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectorError {
    #[error("WebSocket connection failed: {source}")]
    WebSocket {
        #[source]
        source: tungstenite::Error,
        endpoint: String,
        attempt: u32,
    },

    #[error("Failed to parse message: {source}")]
    Parse {
        #[source]
        source: serde_json::Error,
        raw_message: String,
        symbol: Option<String>,
    },

    #[error("Transport publish failed: {source}")]
    Transport {
        #[source]
        source: TransportError,
        subject: String,
    },

    #[error("Configuration error: {message}")]
    Config { message: String },
}

impl ConnectorError {
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::WebSocket { .. } | Self::Transport { .. })
    }
}
```

### Graceful Degradation

When non-critical components fail:

| Component Failure | Degradation |
|-------------------|-------------|
| Cache unavailable | Bypass cache, query DB directly (slower) |
| Archiver behind | Continue streaming, archiver catches up |
| Metadata API down | Use cached metadata, warn on stale |
| One symbol fails | Continue other symbols, log gap |

### Alerting Integration

Errors surface as metrics and alerts:

```yaml
# Error rate alert
- alert: HighErrorRate
  expr: rate(ssmd_errors_total[5m]) > 0.1
  for: 2m
  labels:
    severity: warning
  annotations:
    summary: "High error rate in {{ $labels.component }}"

# Circuit breaker open
- alert: CircuitBreakerOpen
  expr: ssmd_circuit_breaker_state == 1
  for: 1m
  labels:
    severity: critical
  annotations:
    summary: "Circuit breaker open for {{ $labels.target }}"

# Dead letters accumulating
- alert: DeadLettersAccumulating
  expr: increase(ssmd_dead_letters_total[1h]) > 100
  labels:
    severity: warning
```

## Backpressure & Slow Consumers

The system handles consumers that can't keep up without losing data or blocking producers.

### Design Principles

1. **Never block producers** - Connector must keep ingesting exchange data
2. **Bound memory** - Per-client buffers have limits
3. **Detect early** - Identify slow consumers before they cause problems
4. **Degrade gracefully** - Slow consumers get dropped, not crashed

### Architecture

```
                                    ┌─────────────────┐
                                    │  Fast Client A  │◀── Full stream
                                    └─────────────────┘
┌───────────┐     ┌──────────┐     ┌─────────────────┐
│ Connector │────▶│   NATS   │────▶│  Slow Client B  │◀── Buffered, then dropped
└───────────┘     │ JetStream│     └─────────────────┘
                  └──────────┘     ┌─────────────────┐
                                   │  Client C (sub) │◀── Conflated snapshots
                                   └─────────────────┘
```

### NATS JetStream Configuration

JetStream provides durable streams with configurable consumer policies:

```yaml
# Stream configuration
streams:
  ssmd-kalshi:
    subjects:
      - "kalshi.>"
    retention: limits
    max_bytes: 10GB           # Bound total stream size
    max_age: 24h              # Auto-expire old messages
    max_msg_size: 1MB
    discard: old              # Drop oldest when full (not new)
    duplicate_window: 2m      # Dedup window

# Consumer configuration (per client type)
consumers:
  realtime:
    ack_policy: none          # Fire and forget for speed
    max_deliver: 1
    flow_control: true
    idle_heartbeat: 30s

  durable:
    ack_policy: explicit      # Guaranteed delivery
    max_deliver: 5
    ack_wait: 30s
    max_ack_pending: 1000     # Backpressure threshold
```

### Gateway Client Management

Each WebSocket client has a bounded buffer:

```rust
pub struct ClientConnection {
    id: ClientId,
    socket: WebSocketSender,
    buffer: BoundedBuffer,
    subscriptions: HashSet<String>,
    stats: ClientStats,
    state: ClientState,
}

pub struct BoundedBuffer {
    queue: VecDeque<Message>,
    max_size: usize,           // Max messages
    max_bytes: usize,          // Max total bytes
    current_bytes: usize,
    drop_policy: DropPolicy,
}

pub enum DropPolicy {
    DropOldest,                // Drop head of queue
    DropNewest,                // Drop incoming message
    Disconnect,                // Terminate slow client
}

pub struct ClientStats {
    connected_at: Instant,
    messages_sent: u64,
    messages_dropped: u64,
    bytes_sent: u64,
    last_message_at: Instant,
    lag_ms: AtomicU64,
}

pub enum ClientState {
    Healthy,
    Lagging { since: Instant },
    Dropping { dropped: u64 },
    Disconnecting { reason: String },
}
```

### Slow Consumer Detection

Detect slow consumers before buffers fill:

```rust
impl Gateway {
    async fn monitor_clients(&self) {
        loop {
            for client in self.clients.iter() {
                let stats = client.stats();
                let buffer_pct = client.buffer.utilization();

                // Update lag metric
                if let Some(last_seq) = client.last_ack_sequence {
                    let current_seq = self.stream.last_sequence();
                    let lag = current_seq - last_seq;
                    client.stats.lag_ms.store(lag * AVG_MSG_INTERVAL_MS, Ordering::Relaxed);
                }

                // State transitions
                match client.state {
                    ClientState::Healthy if buffer_pct > 0.7 => {
                        client.set_state(ClientState::Lagging { since: Instant::now() });
                        self.metrics.slow_consumers.inc();
                        warn!(client_id = %client.id, buffer_pct, "Client lagging");
                    }
                    ClientState::Lagging { since } if buffer_pct > 0.9 => {
                        client.set_state(ClientState::Dropping { dropped: 0 });
                        warn!(client_id = %client.id, "Client buffer full, dropping messages");
                    }
                    ClientState::Dropping { dropped } if dropped > 1000 => {
                        client.set_state(ClientState::Disconnecting {
                            reason: "Too many dropped messages".into()
                        });
                        warn!(client_id = %client.id, dropped, "Disconnecting slow client");
                    }
                    ClientState::Lagging { .. } if buffer_pct < 0.5 => {
                        client.set_state(ClientState::Healthy);
                        info!(client_id = %client.id, "Client recovered");
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
```

### Conflation for Slow Consumers

Optionally, slow consumers can receive conflated snapshots instead of every tick:

```rust
pub enum SubscriptionMode {
    /// Every message, drop if slow
    Realtime,

    /// Periodic snapshots, never drop
    Conflated { interval: Duration },

    /// Latest value only, overwrite on each update
    Latest,
}

pub struct ConflatedState {
    last_trade: HashMap<String, Trade>,
    orderbook: HashMap<String, OrderBook>,
    last_sent: Instant,
}

impl Gateway {
    async fn send_to_client(&self, client: &mut ClientConnection, msg: Message) {
        match client.subscription_mode {
            SubscriptionMode::Realtime => {
                if client.buffer.try_push(msg).is_err() {
                    client.stats.messages_dropped += 1;
                }
            }
            SubscriptionMode::Conflated { interval } => {
                // Update conflated state
                client.conflated.update(&msg);

                // Send snapshot on interval
                if client.conflated.last_sent.elapsed() >= interval {
                    let snapshot = client.conflated.snapshot();
                    client.socket.send(snapshot).await;
                    client.conflated.last_sent = Instant::now();
                }
            }
            SubscriptionMode::Latest => {
                client.conflated.update(&msg);
                // Only send on explicit request
            }
        }
    }
}
```

### Client Subscription API

Clients choose their mode:

```json
// Realtime (default)
{"action": "subscribe", "symbols": ["BTCUSD"], "mode": "realtime"}

// Conflated every 100ms
{"action": "subscribe", "symbols": ["BTCUSD"], "mode": "conflated", "interval_ms": 100}

// Latest only (poll-based)
{"action": "subscribe", "symbols": ["BTCUSD"], "mode": "latest"}

// Get current snapshot
{"action": "snapshot", "symbols": ["BTCUSD"]}
```

### Metrics

```prometheus
# Per-client buffer utilization
ssmd_gateway_client_buffer_utilization{client_id="abc123"} 0.45

# Slow consumer count
ssmd_gateway_slow_consumers 2

# Messages dropped due to backpressure
ssmd_gateway_messages_dropped_total{reason="buffer_full"} 1523

# Client lag in milliseconds
ssmd_gateway_client_lag_ms{client_id="abc123"} 250

# Disconnections due to slow consumption
ssmd_gateway_disconnections_total{reason="slow_consumer"} 5
```

### CLI for Client Management

```bash
# List connected clients
ssmd client list
# ID          STATE    BUFFER  LAG     SUBSCRIPTIONS
# abc123      healthy  12%     50ms    BTCUSD, ETHUSD
# def456      lagging  78%     2500ms  *
# ghi789      dropping 95%     8000ms  BTCUSD

# Get client details
ssmd client show abc123

# Force disconnect a client
ssmd client disconnect def456 --reason "manual intervention"

# Set client to conflated mode
ssmd client set-mode ghi789 --mode conflated --interval 500ms
```

## Agent Integration

AI agents (Claude, custom bots) interact with ssmd through structured APIs. The system is designed to be agent-friendly: queryable, explainable, and actionable.

### Integration Points

```
┌─────────────────────────────────────────────────────────────────────┐
│                           AI AGENT                                   │
│  (Claude Code, Custom Bot, Notebook)                                │
└───────────┬──────────────────┬──────────────────┬───────────────────┘
            │                  │                  │
            ▼                  ▼                  ▼
     ┌─────────────┐   ┌─────────────┐   ┌─────────────┐
     │  MCP Server │   │   REST API  │   │  WebSocket  │
     │  (tools)    │   │  (queries)  │   │  (stream)   │
     └──────┬──────┘   └──────┬──────┘   └──────┬──────┘
            │                 │                 │
            └────────────────┼─────────────────┘
                             │
                      ┌──────▼──────┐
                      │   Gateway   │
                      └─────────────┘
```

### MCP Server (ssmd-mcp)

Model Context Protocol server exposes ssmd as tools for Claude:

```go
// ssmd-mcp implements MCP server protocol
type SSMDServer struct {
    gateway  *GatewayClient
    metadata *MetadataClient
}

// Tools exposed to Claude
var Tools = []mcp.Tool{
    {
        Name:        "ssmd_list_markets",
        Description: "List available markets with optional filters",
        InputSchema: schema.Object{
            "feed":     schema.String{Description: "Filter by feed (kalshi, polymarket)"},
            "status":   schema.String{Description: "Filter by status (active, expired)"},
            "category": schema.String{Description: "Filter by category"},
        },
    },
    {
        Name:        "ssmd_get_market",
        Description: "Get details for a specific market including current price",
        InputSchema: schema.Object{
            "ticker": schema.String{Required: true, Description: "Market ticker"},
        },
    },
    {
        Name:        "ssmd_get_trades",
        Description: "Get recent trades for a market",
        InputSchema: schema.Object{
            "ticker": schema.String{Required: true},
            "limit":  schema.Integer{Default: 100, Max: 1000},
            "since":  schema.String{Description: "ISO timestamp"},
        },
    },
    {
        Name:        "ssmd_get_orderbook",
        Description: "Get current orderbook for a market",
        InputSchema: schema.Object{
            "ticker": schema.String{Required: true},
            "depth":  schema.Integer{Default: 10, Max: 50},
        },
    },
    {
        Name:        "ssmd_query_historical",
        Description: "Query historical data for backtesting",
        InputSchema: schema.Object{
            "ticker":     schema.String{Required: true},
            "start_date": schema.String{Required: true, Description: "YYYY-MM-DD"},
            "end_date":   schema.String{Required: true, Description: "YYYY-MM-DD"},
            "interval":   schema.String{Default: "1m", Description: "1m, 5m, 1h, 1d"},
        },
    },
    {
        Name:        "ssmd_report_issue",
        Description: "Report a data quality issue for investigation",
        InputSchema: schema.Object{
            "ticker":      schema.String{Required: true},
            "issue_type":  schema.String{Required: true, Enum: []string{"missing_data", "incorrect_price", "duplicate", "other"}},
            "description": schema.String{Required: true},
            "timestamp":   schema.String{Description: "When the issue occurred"},
            "evidence":    schema.String{Description: "Supporting data or observations"},
        },
    },
    {
        Name:        "ssmd_system_status",
        Description: "Get current system health and data coverage",
        InputSchema: schema.Object{},
    },
    {
        Name:        "ssmd_data_inventory",
        Description: "Check what data is available for a date range",
        InputSchema: schema.Object{
            "feed":       schema.String{Required: true},
            "start_date": schema.String{Required: true},
            "end_date":   schema.String{Required: true},
        },
    },
}
```

### Agent-Friendly Responses

Responses include context that helps agents understand and act:

```json
// Response to ssmd_get_market
{
  "ticker": "INXD-25-B4000",
  "title": "Will S&P 500 close above 4000 on Dec 31, 2025?",
  "feed": "kalshi",
  "status": "active",
  "current_price": {
    "yes": 0.45,
    "no": 0.55,
    "last_trade": 0.45,
    "last_trade_time": "2025-12-14T10:30:00Z"
  },
  "orderbook_summary": {
    "best_bid": 0.44,
    "best_ask": 0.46,
    "spread": 0.02,
    "bid_depth": 1500,
    "ask_depth": 2000
  },
  "contract": {
    "expiration": "2025-12-31T23:59:59Z",
    "settlement": "2026-01-01T12:00:00Z",
    "days_to_expiry": 17
  },
  "data_quality": {
    "status": "healthy",
    "last_update": "2025-12-14T10:30:01Z",
    "gaps_today": 0
  },
  "_links": {
    "trades": "/v1/markets/INXD-25-B4000/trades",
    "orderbook": "/v1/markets/INXD-25-B4000/orderbook",
    "historical": "/v1/markets/INXD-25-B4000/history"
  },
  "_hints": {
    "price_interpretation": "0.45 yes price implies 45% probability of S&P > 4000",
    "suggested_actions": [
      "Use ssmd_get_trades to see recent activity",
      "Use ssmd_query_historical for trend analysis"
    ]
  }
}
```

### Agent Feedback Loop

Agents can report data quality issues that feed back into the system:

```sql
CREATE TABLE agent_feedback (
  id SERIAL PRIMARY KEY,
  agent_id VARCHAR(64),            -- MCP client identifier
  ticker VARCHAR(64),
  issue_type VARCHAR(32) NOT NULL,
  description TEXT NOT NULL,
  timestamp TIMESTAMPTZ,
  evidence JSONB,

  -- Triage
  status VARCHAR(16) DEFAULT 'open',  -- open, investigating, resolved, invalid
  priority VARCHAR(16),
  assigned_to VARCHAR(64),

  -- Resolution
  resolution TEXT,
  resolved_at TIMESTAMPTZ,

  created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_agent_feedback_status ON agent_feedback(status);
CREATE INDEX idx_agent_feedback_ticker ON agent_feedback(ticker);
```

Feedback workflow:

```
Agent reports issue
       │
       ▼
┌─────────────────┐
│ Create feedback │
│ record (open)   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│ Auto-triage:    │────▶│ Link to existing│
│ duplicate?      │ yes │ issue           │
└────────┬────────┘     └─────────────────┘
         │ no
         ▼
┌─────────────────┐     ┌─────────────────┐
│ Auto-validate:  │────▶│ Mark invalid,   │
│ data exists?    │ no  │ notify agent    │
└────────┬────────┘     └─────────────────┘
         │ yes
         ▼
┌─────────────────┐
│ Create Linear   │
│ issue (if high  │
│ priority)       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Investigate &   │
│ resolve         │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Update agent    │
│ via webhook     │
└─────────────────┘
```

### Natural Language Queries

Gateway supports natural language queries that get translated to structured queries:

```json
// Agent sends
{
  "action": "query",
  "natural_language": "What prediction markets about Bitcoin are trading today?"
}

// Gateway translates to
{
  "action": "list_markets",
  "filters": {
    "category": "crypto",
    "underlying": "BTC",
    "status": "active"
  }
}

// And returns
{
  "interpretation": "Searching for active markets related to Bitcoin",
  "results": [...],
  "suggestions": [
    "To narrow down: 'Bitcoin price markets expiring this week'",
    "For specific market: 'ssmd_get_market KXBTC-25DEC31'"
  ]
}
```

### Rate Limiting for Agents

Agents have separate rate limits from real-time streaming:

```yaml
rate_limits:
  agents:
    requests_per_minute: 60
    requests_per_hour: 1000
    burst: 10

  # Higher limits for feedback (we want bug reports)
  feedback:
    requests_per_minute: 10
    requests_per_hour: 100
```

### Claude Code Integration

Example Claude Code session:

```
Human: What's the current price of the S&P 4000 prediction market on Kalshi?

Claude: I'll check the current market data.

[Calls ssmd_get_market with ticker pattern matching "S&P 4000"]

The S&P 500 above 4000 market (INXD-25-B4000) is currently trading at:
- Yes: $0.45 (45% implied probability)
- No: $0.55
- Spread: $0.02

The market expires on Dec 31, 2025 (17 days). Last trade was 2 minutes ago.
```

## Testing Strategy

Testing ensures correctness without a QA team. The system tests itself through automation, replay, and comparison.

### Testing Layers

```
┌─────────────────────────────────────────────────────────────────────┐
│                        PRODUCTION                                    │
│   Real feeds, real data, real users                                 │
└─────────────────────────────────────────────────────────────────────┘
                              ▲
┌─────────────────────────────────────────────────────────────────────┐
│                     REPLAY TESTING                                   │
│   Historical data, production code, automated comparison            │
└─────────────────────────────────────────────────────────────────────┘
                              ▲
┌─────────────────────────────────────────────────────────────────────┐
│                   INTEGRATION TESTING                                │
│   In-memory middleware, real components, docker-compose             │
└─────────────────────────────────────────────────────────────────────┘
                              ▲
┌─────────────────────────────────────────────────────────────────────┐
│                      UNIT TESTING                                    │
│   Isolated functions, mocked dependencies, fast feedback            │
└─────────────────────────────────────────────────────────────────────┘
```

### Unit Tests

Fast, isolated tests for individual functions:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kalshi_trade() {
        let raw = r#"{"type":"trade","ticker":"INXD-25-B4000","price":0.45,"count":10}"#;
        let trade = parse_kalshi_message(raw).unwrap();

        assert!(matches!(trade, KalshiMessage::Trade(_)));
        if let KalshiMessage::Trade(t) = trade {
            assert_eq!(t.ticker, "INXD-25-B4000");
            assert_eq!(t.price, 0.45);
        }
    }

    #[test]
    fn test_capnp_roundtrip() {
        let trade = Trade {
            timestamp: 1702540800000,
            ticker: "BTCUSD".into(),
            price: 45000.0,
            size: 100,
            side: Side::Buy,
            trade_id: "abc123".into(),
        };

        let encoded = trade.to_capnp();
        let decoded = Trade::from_capnp(&encoded).unwrap();

        assert_eq!(trade, decoded);
    }

    #[test]
    fn test_retry_policy_backoff() {
        let policy = RetryPolicy::default();
        let delays: Vec<_> = (0..5).map(|i| policy.delay_for_attempt(i)).collect();

        // Should be exponential with jitter
        assert!(delays[1] > delays[0]);
        assert!(delays[2] > delays[1]);
        assert!(delays[4] <= policy.max_delay);
    }

    #[test]
    fn test_bounded_buffer_drop_oldest() {
        let mut buffer = BoundedBuffer::new(3, DropPolicy::DropOldest);

        buffer.push(msg(1));
        buffer.push(msg(2));
        buffer.push(msg(3));
        buffer.push(msg(4));  // Should drop msg(1)

        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.pop().unwrap().sequence, 2);
    }
}
```

### Integration Tests

Tests with real components but in-memory middleware:

```rust
#[tokio::test]
async fn test_connector_to_gateway_flow() {
    // Setup in-memory middleware
    let transport = Arc::new(InMemoryTransport::new());
    let storage = Arc::new(InMemoryStorage::new());

    // Create components
    let connector = Connector::new(
        MockKalshiClient::new(sample_messages()),
        transport.clone(),
    );
    let gateway = Gateway::new(transport.clone());

    // Start components
    let connector_handle = tokio::spawn(connector.run());
    let gateway_handle = tokio::spawn(gateway.run());

    // Connect a test client
    let mut client = gateway.connect_test_client().await;
    client.subscribe(&["INXD-25-B4000"]).await;

    // Wait for messages to flow
    let msg = timeout(Duration::from_secs(5), client.next()).await.unwrap();

    assert!(matches!(msg, GatewayMessage::Trade(_)));

    // Cleanup
    connector_handle.abort();
    gateway_handle.abort();
}

#[tokio::test]
async fn test_archiver_writes_to_storage() {
    let transport = Arc::new(InMemoryTransport::new());
    let storage = Arc::new(InMemoryStorage::new());

    // Publish test messages
    for i in 0..100 {
        transport.publish("kalshi.trade.BTCUSD", sample_trade(i)).await;
    }

    // Run archiver
    let archiver = Archiver::new(transport.clone(), storage.clone());
    archiver.flush().await;

    // Verify storage
    let files = storage.list("ssmd-raw", "kalshi/").await.unwrap();
    assert!(!files.is_empty());

    let content = storage.get("ssmd-raw", &files[0].key).await.unwrap();
    assert!(content.len() > 0);
}
```

### Docker Compose for Local Integration

```yaml
# docker-compose.test.yaml
version: '3.8'

services:
  nats:
    image: nats:2.10
    command: ["--jetstream"]
    ports:
      - "4222:4222"

  postgres:
    image: postgres:16
    environment:
      POSTGRES_DB: ssmd_test
      POSTGRES_USER: ssmd
      POSTGRES_PASSWORD: test
    ports:
      - "5432:5432"

  minio:
    image: minio/minio
    command: server /data
    environment:
      MINIO_ROOT_USER: minioadmin
      MINIO_ROOT_PASSWORD: minioadmin
    ports:
      - "9000:9000"

  redis:
    image: redis:7
    ports:
      - "6379:6379"
```

```bash
# Run integration tests
docker-compose -f docker-compose.test.yaml up -d
cargo test --features integration
docker-compose -f docker-compose.test.yaml down
```

### Replay Testing

Compare new code against recorded production data:

```rust
pub struct ReplayTest {
    date: NaiveDate,
    feed: String,
    baseline_version: String,
    candidate_version: String,
}

impl ReplayTest {
    pub async fn run(&self) -> ReplayReport {
        // Load raw data from storage
        let raw_data = self.load_raw_data().await;

        // Process with baseline version
        let baseline_output = self.process_with_version(&self.baseline_version, &raw_data).await;

        // Process with candidate version
        let candidate_output = self.process_with_version(&self.candidate_version, &raw_data).await;

        // Compare outputs
        let diff = self.compare_outputs(&baseline_output, &candidate_output);

        ReplayReport {
            date: self.date,
            feed: self.feed.clone(),
            baseline_count: baseline_output.len(),
            candidate_count: candidate_output.len(),
            differences: diff,
            passed: diff.is_empty(),
        }
    }

    fn compare_outputs(&self, baseline: &[NormalizedMessage], candidate: &[NormalizedMessage]) -> Vec<Difference> {
        let mut diffs = Vec::new();

        // Check count
        if baseline.len() != candidate.len() {
            diffs.push(Difference::CountMismatch {
                baseline: baseline.len(),
                candidate: candidate.len(),
            });
        }

        // Check content (allowing for timestamp tolerance)
        for (b, c) in baseline.iter().zip(candidate.iter()) {
            if !messages_equal(b, c, Duration::from_millis(1)) {
                diffs.push(Difference::ContentMismatch {
                    baseline: b.clone(),
                    candidate: c.clone(),
                });
            }
        }

        diffs
    }
}
```

CLI for replay testing:

```bash
# Replay single day
ssmd test replay --feed kalshi --date 2025-12-14 \
  --baseline v1.2.3 --candidate v1.2.4

# Replay date range
ssmd test replay --feed kalshi \
  --from 2025-12-01 --to 2025-12-14 \
  --baseline v1.2.3 --candidate v1.2.4

# Output
Replay Test Report
==================
Feed: kalshi
Date Range: 2025-12-01 to 2025-12-14
Baseline: v1.2.3
Candidate: v1.2.4

Date        Baseline    Candidate   Status
2025-12-01  1,234,567   1,234,567   PASS
2025-12-02  1,245,678   1,245,678   PASS
2025-12-03  1,256,789   1,256,792   FAIL (3 diffs)
...

Failures:
2025-12-03:
  - Message #45678: price 0.450 vs 0.451 (rounding change?)
  - Message #45679: missing in candidate
  - Message #45680: missing in candidate
```

### Automated QA Pipeline

GitHub Actions workflow for continuous testing:

```yaml
# .github/workflows/test.yaml
name: Test

on:
  push:
    branches: [main]
  pull_request:

jobs:
  unit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --lib

  integration:
    runs-on: ubuntu-latest
    services:
      nats:
        image: nats:2.10
        options: --entrypoint "nats-server --jetstream"
      postgres:
        image: postgres:16
        env:
          POSTGRES_DB: ssmd_test
          POSTGRES_USER: ssmd
          POSTGRES_PASSWORD: test
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --features integration

  replay:
    runs-on: ubuntu-latest
    if: github.event_name == 'pull_request'
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      # Download sample data from S3
      - name: Download test data
        run: |
          aws s3 sync s3://ssmd-test-data/replay ./test-data

      # Run replay against last 7 days
      - name: Replay test
        run: |
          cargo run --release -- test replay \
            --feed kalshi \
            --from $(date -d '7 days ago' +%Y-%m-%d) \
            --to $(date +%Y-%m-%d) \
            --baseline ${{ github.base_ref }} \
            --candidate ${{ github.head_ref }}

  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
```

### Property-Based Testing

Use proptest for edge cases:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_capnp_roundtrip_any_trade(
        timestamp in 0u64..u64::MAX,
        ticker in "[A-Z]{3,10}",
        price in 0.0f64..1000000.0,
        size in 0u32..u32::MAX,
    ) {
        let trade = Trade {
            timestamp,
            ticker,
            price,
            size,
            side: Side::Buy,
            trade_id: "test".into(),
        };

        let encoded = trade.to_capnp();
        let decoded = Trade::from_capnp(&encoded).unwrap();

        prop_assert_eq!(trade.timestamp, decoded.timestamp);
        prop_assert_eq!(trade.ticker, decoded.ticker);
        prop_assert!((trade.price - decoded.price).abs() < 0.0001);
        prop_assert_eq!(trade.size, decoded.size);
    }

    #[test]
    fn test_bounded_buffer_never_exceeds_capacity(
        ops in prop::collection::vec(0u8..2, 0..1000),
        capacity in 1usize..100,
    ) {
        let mut buffer = BoundedBuffer::new(capacity, DropPolicy::DropOldest);

        for op in ops {
            match op {
                0 => { buffer.push(Message::default()); }
                1 => { buffer.pop(); }
                _ => {}
            }
            prop_assert!(buffer.len() <= capacity);
        }
    }
}
```

### Chaos Testing

Inject failures to verify resilience:

```rust
pub struct ChaosConfig {
    pub network_failure_rate: f64,      // 0.0 to 1.0
    pub latency_injection_ms: u64,
    pub message_corruption_rate: f64,
    pub connection_drop_interval: Duration,
}

pub struct ChaosTransport {
    inner: Arc<dyn Transport>,
    config: ChaosConfig,
}

#[async_trait]
impl Transport for ChaosTransport {
    async fn publish(&self, subject: &str, payload: Bytes) -> Result<(), TransportError> {
        // Random failure
        if rand::random::<f64>() < self.config.network_failure_rate {
            return Err(TransportError::NetworkFailure);
        }

        // Latency injection
        if self.config.latency_injection_ms > 0 {
            tokio::time::sleep(Duration::from_millis(
                rand::thread_rng().gen_range(0..self.config.latency_injection_ms)
            )).await;
        }

        self.inner.publish(subject, payload).await
    }
}
```

```bash
# Run with chaos enabled
SSMD_CHAOS_NETWORK_FAILURE=0.05 \
SSMD_CHAOS_LATENCY_MS=100 \
cargo test --features chaos
```

### Test Data Management

Fixtures and sample data:

```
test-data/
├── fixtures/
│   ├── kalshi/
│   │   ├── trade.json
│   │   ├── orderbook.json
│   │   └── market_status.json
│   └── schemas/
│       └── trade_v1.capnp
├── replay/
│   └── kalshi/
│       └── 2025-12-14/
│           ├── raw.jsonl.zst
│           └── expected.capnp.zst
└── golden/
    └── kalshi/
        └── trade_normalization.json
```

Golden tests for output stability:

```rust
#[test]
fn test_trade_normalization_golden() {
    let input = include_str!("../test-data/fixtures/kalshi/trade.json");
    let expected = include_str!("../test-data/golden/kalshi/trade_normalization.json");

    let result = normalize_kalshi_trade(input).unwrap();
    let result_json = serde_json::to_string_pretty(&result).unwrap();

    assert_eq!(result_json, expected);
}
```

### Coverage Requirements

```toml
# .cargo/config.toml
[build]
rustflags = ["-C", "instrument-coverage"]

# Minimum coverage thresholds
[coverage]
line = 80
branch = 70
function = 90
```

```bash
# Generate coverage report
cargo llvm-cov --html --output-dir coverage/
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

### Rust Crates

| Dependency | Version | Purpose |
|------------|---------|---------|
| tokio | 1.x | Async runtime |
| tungstenite | 0.21 | WebSocket client |
| capnp | 0.18 | Cap'n Proto |
| async-nats | 0.33 | NATS client |
| sqlx | 0.7 | PostgreSQL |
| serde | 1.x | JSON serialization |
| tracing | 0.1 | Structured logging |

### Go Modules

| Dependency | Purpose |
|------------|---------|
| github.com/spf13/cobra | CLI framework |
| github.com/temporalio/sdk-go | Temporal workflows |
| github.com/nats-io/nats.go | NATS client |
| github.com/jackc/pgx/v5 | PostgreSQL driver |
| gopkg.in/yaml.v3 | YAML parsing |

## Infrastructure Requirements

### Existing in varlab (ready to use)

| Service | Version | Location | Notes |
|---------|---------|----------|-------|
| NATS + JetStream | 2.12.2 | `infrastructure/nats/` | File persistence, KALSHI_TRADES stream exists |
| PostgreSQL | 16 | `infrastructure/authentik/` | Create `ssmd` database |
| Redis | 8.2.1 | `infrastructure/authentik/` | May need dedicated instance for ssmd |
| Sealed Secrets | 2.15.0+ | `infrastructure/sealed-secrets/` | Ready for ssmd secrets |
| Traefik | 37.3.0 | `infrastructure/traefik/` | Ingress with TLS |
| Longhorn | 1.7.2 | Storage class | Block storage for PVCs |
| Prometheus/Grafana/Loki | - | `infrastructure/observability/` | Monitoring stack |

### Needs Deployment

| Service | Purpose | Priority |
|---------|---------|----------|
| **ArgoCD** | GitOps deployment for ssmd (separate from Flux) | Phase 0 |
| **Temporal** | Workflow orchestration for daily startup/teardown | Phase 0 |

### Deferred

| Service | Purpose | Notes |
|---------|---------|-------|
| **Garage** | S3-compatible object storage | Deferred until Brooklyn NAS build |

### Storage Strategy (Pre-Garage)

Until Garage is deployed on the Brooklyn NAS:

1. **Raw data**: Local PVC via Longhorn (limited retention)
2. **Normalized data**: Local PVC via Longhorn
3. **Backups**: Manual export to external storage

The Storage trait abstraction allows seamless migration to Garage when ready:

```yaml
# Initial: Local storage
middleware:
  storage:
    type: local
    path: /var/lib/ssmd/storage

# Future: Garage S3
middleware:
  storage:
    type: s3
    endpoint: http://garage.brooklyn.local:3900
    buckets:
      raw: ssmd-raw
      normalized: ssmd-normalized
```

### Infrastructure Setup (Phase 0)

Before application development:

```bash
# 1. Deploy ArgoCD to homelab
kubectl create namespace argocd
kubectl apply -n argocd -f https://raw.githubusercontent.com/argoproj/argo-cd/stable/manifests/install.yaml

# 2. Configure ArgoCD for ssmd repo
argocd app create ssmd \
  --repo https://github.com/your-org/ssmd.git \
  --path k8s/overlays/prod \
  --dest-server https://kubernetes.default.svc \
  --dest-namespace ssmd

# 3. Deploy Temporal
helm repo add temporal https://temporal.io/helm-charts
helm install temporal temporal/temporal \
  --namespace temporal \
  --create-namespace \
  --set server.replicaCount=1 \
  --set cassandra.enabled=false \
  --set postgresql.enabled=true \
  --set prometheus.enabled=true

# 4. Create ssmd database in existing PostgreSQL
kubectl exec -it postgresql-0 -n authentik -- psql -U postgres -c "CREATE DATABASE ssmd;"

# 5. Create ssmd namespace and sealed secrets
kubectl create namespace ssmd
kubeseal --fetch-cert > ssmd-sealed-secrets-cert.pem
```

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
