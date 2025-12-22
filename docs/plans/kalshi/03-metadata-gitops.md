# ssmd: Kalshi Design - Metadata (GitOps)

Metadata is the foundation for operator safety. The system is self-describing: every feed, symbol, schema version, and data file is tracked. Operators cannot misconfigure what they can query.

## Design Principles

1. **Query before act** - CLI validates against metadata before any operation
2. **No implicit state** - All configuration is explicit and versioned
3. **Fail fast** - Invalid references fail at config time, not runtime
4. **Time-travel** - All metadata is temporal; query state as-of any date
5. **Git is the source of truth** - No database for metadata; YAML files in git

## Metadata Domains

```
┌─────────────────────────────────────────────────────────────────┐
│                      METADATA (GitOps)                          │
├─────────────────┬─────────────────┬─────────────────────────────┤
│  Feed Registry  │  Data Inventory │  Schema Registry            │
│  exchanges/     │  (runtime only) │  exchanges/                 │
│  feeds/*.yaml   │  S3 manifests   │  schemas/*.yaml             │
├─────────────────┴─────────────────┴─────────────────────────────┤
│                    Environment Configuration                     │
│  exchanges/environments/*.yaml                                   │
└─────────────────────────────────────────────────────────────────┘
```

## 1. Feed Registry

Feeds are defined as YAML files in `exchanges/feeds/`. See [file-formats.md](../reference/file-formats.md) for complete specification.

```yaml
# exchanges/feeds/kalshi.yaml
name: kalshi
display_name: Kalshi Exchange
type: websocket
status: active

versions:
  - version: v2
    effective_from: 2025-01-01
    protocol: wss
    endpoint: wss://api.kalshi.com/trade-api/ws/v2
    auth_method: api_key
    rate_limit_per_second: 10
    max_symbols_per_connection: 100
    supports_orderbook: true
    supports_trades: true

calendar:
  timezone: America/New_York
  holiday_calendar: us_equity
```

### CLI Operations

```bash
# Create a new feed
ssmd feed create polymarket \
  --type websocket \
  --endpoint 'wss://ws-subscriptions-clob.polymarket.com/ws/market'

# Add a new version to existing feed
ssmd feed add-version kalshi \
  --version v3 \
  --effective-from 2025-07-01 \
  --endpoint 'wss://api.kalshi.com/trade-api/ws/v3'

# Show feed configuration as-of a date
ssmd feed show kalshi --as-of 2025-12-01

# List all active feeds
ssmd feed list --status active
```

### Version Resolution

Given a date, the system uses the version where:
- `effective_from <= date`
- No other version has a later `effective_from` that is still `<= date`

This enables reproducible backtesting: replay Dec 1st data with the exact configuration that was active on Dec 1st.

## 2. Schema Registry

Schemas have two files:
- `exchanges/schemas/<name>.capnp` — Cap'n Proto definition
- `exchanges/schemas/<name>.yaml` — Version metadata

```yaml
# exchanges/schemas/trade.yaml
name: trade
format: capnp
schema_file: trade.capnp

versions:
  - version: v1
    effective_from: 2025-01-01
    status: active
    hash: sha256:a1b2c3d4e5f6789...

  - version: v2
    effective_from: 2025-06-01
    status: draft
    hash: sha256:f6e5d4c3b2a1098...
    compatible_with: [v1]
    breaking_changes: "Added takerSide field"
```

### Status Lifecycle

```
draft ──> active ──> deprecated
```

- `draft` — Work in progress. Cannot be used in production environments.
- `active` — Current version. Can be referenced by environments.
- `deprecated` — Still valid but should be migrated away from.

### CLI Operations

```bash
# Register a new schema
ssmd schema register trade --file schemas/trade.capnp

# Set status to active
ssmd schema set-status trade:v1 active

# Compute and verify hash
ssmd schema hash trade

# Show schema for a specific version
ssmd schema show trade:v1

# List all schemas
ssmd schema list
```

### Hash Integrity

The CLI computes SHA256 hash from the schema file. Validation fails if stored hash doesn't match computed hash — this catches accidental schema file modifications.

## 3. Environment Configuration

Environments tie together feeds, schemas, and infrastructure. See [file-formats.md](../reference/file-formats.md) for complete specification.

```yaml
# exchanges/environments/kalshi-dev.yaml
name: kalshi-dev
feed: kalshi
schema: trade:v1

keys:
  kalshi:
    type: api_key
    required: true
    fields: [api_key, api_secret]
    source: env

transport:
  type: nats
  url: nats://localhost:4222

storage:
  type: local
  path: /var/lib/ssmd/data
```

### CLI Operations

```bash
# Create environment
ssmd env create kalshi-dev --feed kalshi --schema trade:v1

# Update environment
ssmd env update kalshi-dev --schema trade:v2

# Show environment
ssmd env show kalshi-dev

# List environments
ssmd env list
```

## 4. Data Inventory (Runtime)

Unlike feeds, schemas, and environments, data inventory is **not** stored in git. It's computed at runtime from S3 manifest files.

Each day's data directory includes a manifest:

```
s3://ssmd-raw/kalshi/2025/12/14/
├── manifest.json
├── trades_000.capnp.zst
├── trades_001.capnp.zst
└── ...

s3://ssmd-normalized/kalshi/2025/12/14/
├── manifest.json
├── trades.capnp.zst
└── ...
```

```json
// manifest.json
{
  "feed": "kalshi",
  "date": "2025-12-14",
  "data_type": "raw",
  "schema_version": null,
  "status": "complete",
  "record_count": 1234567,
  "byte_size": 523456789,
  "first_timestamp": "2025-12-14T00:00:01.234Z",
  "last_timestamp": "2025-12-14T23:59:59.987Z",
  "gaps": [],
  "quality_score": 1.0,
  "connector_version": "0.1.0",
  "created_at": "2025-12-15T00:05:00Z"
}
```

### CLI Operations

```bash
# Query data inventory (reads manifests from S3)
ssmd data inventory --feed kalshi --from 2025-12-01 --to 2025-12-14

# Show gaps for a specific date
ssmd data gaps --feed kalshi --date 2025-12-14

# Data quality report
ssmd data quality --feed kalshi --date 2025-12-14
```

### Inventory Output

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

## Cross-Reference Validation

The `ssmd validate` command checks referential integrity:

```bash
$ ssmd validate

Validating feeds...
  ✓ kalshi: valid

Validating schemas...
  ✓ trade: v1 hash matches

Validating environments...
  ✓ kalshi-dev: references valid feed and schema

All validations passed.
```

### Validation Rules

- Environment `feed` must reference existing feed file
- Environment `schema` must reference existing schema with `active` status
- Schema `hash` must match computed hash from schema file
- No circular dependencies

## Temporal Queries (As-Of)

Version arrays with `effective_from` dates enable temporal queries:

```bash
# What was the Kalshi endpoint on Dec 1st?
ssmd feed show kalshi --as-of 2025-12-01

# What schema version should be used for Dec 14th data?
ssmd schema show trade --as-of 2025-12-14
```

The CLI resolves the correct version by finding the latest `effective_from <= query_date`.

## Git Workflow

All metadata changes go through git:

```bash
# Make changes
ssmd feed add-version kalshi --version v3 ...

# Review changes
ssmd diff

# Commit
ssmd commit -m "Add Kalshi v3 endpoint"

# Push for review
git push origin feature/kalshi-v3
```

The `ssmd diff` and `ssmd commit` commands are convenience wrappers around git that validate before committing.

## Why GitOps (No Database)

1. **Simplicity** - No database to deploy, backup, or manage
2. **Version control** - Full history with git blame, bisect, revert
3. **Code review** - All changes go through PR review
4. **Reproducibility** - Clone repo = full metadata state
5. **Offline development** - No database connection needed
6. **Disaster recovery** - Repo is the backup

The only runtime state is data inventory manifests in S3, which are computed from the actual data files.
