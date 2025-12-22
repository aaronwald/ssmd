# File Format Reference

> Detailed specifications for ssmd configuration files.

## Feed Files

Location: `feeds/<name>.yaml`

### Example

```yaml
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
    supports_historical: false
    parser_config:
      message_format: json
      timestamp_field: ts

calendar:
  timezone: America/New_York
  holiday_calendar: us_equity
  open_time: "04:00"
  close_time: "00:00"
```

### Fields

#### Root Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Unique identifier. Must match filename. |
| `display_name` | string | no | Human-readable name. |
| `type` | string | yes | `websocket`, `rest`, `multicast` |
| `status` | string | no | `active`, `deprecated`, `disabled`. Default: `active` |
| `versions` | array | yes | Version history. At least one required. |
| `calendar` | object | no | Trading schedule. |

#### Version Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `version` | string | yes | Version identifier (e.g., `v1`, `v2`). |
| `effective_from` | date | yes | When this version takes effect (YYYY-MM-DD). |
| `protocol` | string | yes | `wss`, `https`, `multicast` |
| `endpoint` | string | yes | Connection URL or template. |
| `auth_method` | string | no | `api_key`, `oauth`, `mtls`, `none` |
| `rate_limit_per_second` | integer | no | Max requests per second. |
| `max_symbols_per_connection` | integer | no | Max symbols per single connection. |
| `supports_orderbook` | boolean | no | Feed provides orderbook data. Default: `false` |
| `supports_trades` | boolean | no | Feed provides trade data. Default: `true` |
| `supports_historical` | boolean | no | Feed supports historical queries. Default: `false` |
| `parser_config` | object | no | Feed-specific parsing configuration. |

#### Calendar Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `timezone` | string | no | IANA timezone (e.g., `America/New_York`). |
| `holiday_calendar` | string | no | `us_equity`, `crypto_247`, `custom` |
| `open_time` | string | no | Market open time (HH:MM). |
| `close_time` | string | no | Market close time (HH:MM). |

### Version Resolution

Given a date, the system uses the version where:
- `effective_from <= date`
- No other version has a later `effective_from` that is still `<= date`

Versions are immutable history. To fix a historical version (e.g., for backtesting), use `--version` flag with CLI commands.

---

## Schema Files

Location: `schemas/<name>.capnp` and `schemas/<name>.yaml`

Each schema has two files:
1. **Definition file** (`.capnp`) — The actual Cap'n Proto schema
2. **Metadata file** (`.yaml`) — Version tracking, hashes, compatibility

### Definition Example

```capnp
# schemas/trade.capnp
@0xabcdef1234567890;

struct Trade {
  timestamp @0 :UInt64;
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
```

### Metadata Example

```yaml
# schemas/trade.yaml
name: trade
format: capnp
schema_file: trade.capnp

versions:
  - version: v1
    effective_from: 2025-01-01
    status: active
    hash: sha256:a1b2c3d4e5f6789...
    compatible_with: []

  - version: v2
    effective_from: 2025-06-01
    status: draft
    hash: sha256:f6e5d4c3b2a1098...
    compatible_with: [v1]
    breaking_changes: "Added takerSide field"
```

### Metadata Fields

#### Root Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Unique identifier. Must match filename. |
| `format` | string | yes | `capnp`, `protobuf`, `json_schema` |
| `schema_file` | string | yes | Relative path to definition file. |
| `versions` | array | yes | Version history. At least one required. |

#### Version Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `version` | string | yes | Version identifier (e.g., `v1`, `v2`). |
| `effective_from` | date | yes | When this version takes effect (YYYY-MM-DD). |
| `status` | string | yes | `draft`, `active`, `deprecated` |
| `hash` | string | yes | SHA256 hash of schema file. Computed by CLI. |
| `compatible_with` | array | no | List of older versions that can be auto-converted. |
| `breaking_changes` | string | no | Description of breaking changes from previous version. |

### Status Lifecycle

```
draft ──> active ──> deprecated
```

- `draft` — Work in progress. Cannot be used in production environments.
- `active` — Current version. Can be referenced by environments.
- `deprecated` — Still valid but should be migrated away from.

### Hash Computation

The CLI computes the hash from the schema file:

```bash
ssmd schema hash trade
# Computes SHA256 of schemas/trade.capnp
# Updates hash in schemas/trade.yaml if changed
```

Validation fails if stored hash doesn't match computed hash.

---

## Environment Files

Location: `environments/<name>.yaml`

### Example

```yaml
name: kalshi-dev
feed: kalshi
schema: trade:v1

schedule:
  timezone: UTC
  day_start: "00:10"
  day_end: "00:00"
  auto_roll: true

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

cache:
  type: memory
  max_size: 100MB
```

### Fields

#### Root Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Unique identifier. Must match filename. |
| `feed` | string | yes | Reference to feed in `feeds/`. |
| `schema` | string | yes | Reference to schema as `name:version`. |
| `schedule` | object | no | When to run collection. |
| `keys` | object | no | Key/secret references. |
| `transport` | object | yes | Message transport configuration. |
| `storage` | object | yes | Data storage configuration. |
| `cache` | object | no | Cache configuration. |

#### Schedule Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `timezone` | string | no | IANA timezone for schedule. Default: `UTC` |
| `day_start` | string | no | Time to start collection (HH:MM). |
| `day_end` | string | no | Time to end collection (HH:MM). |
| `auto_roll` | boolean | no | Automatically roll to next day. Default: `true` |

#### Key Fields

Each key is an object with:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | `api_key`, `database`, `transport`, `storage` |
| `required` | boolean | no | Must be set before deployment. Default: `true` |
| `fields` | array | yes | List of field names (e.g., `[api_key, api_secret]`). |
| `source` | string | yes | Where to get the values. See below. |
| `rotation_days` | integer | no | Recommended rotation period. |

#### Key Sources

| Source | Format | Description |
|--------|--------|-------------|
| `env` | — | Environment variables: `SSMD_<KEY>_<FIELD>` |
| `sealed-secret/<name>` | Kubernetes | Sealed secret reference |
| `vault/<path>` | HashiCorp | Vault path |

Example environment variable names for `kalshi` key with fields `[api_key, api_secret]`:
- `SSMD_KALSHI_API_KEY`
- `SSMD_KALSHI_API_SECRET`

#### Transport Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | `nats`, `mqtt`, `memory` |
| `url` | string | varies | Connection URL (required for nats, mqtt). |

#### Storage Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | `local`, `s3` |
| `path` | string | varies | Local directory (required for local). |
| `bucket` | string | varies | S3 bucket name (required for s3). |
| `region` | string | varies | AWS region (required for s3). |

#### Cache Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | `memory`, `redis` |
| `max_size` | string | no | Max cache size (e.g., `100MB`). |
| `url` | string | varies | Redis URL (required for redis). |

---

## Validation Rules

### Feed Validation

- `name` must match filename (without `.yaml`)
- `type` must be one of: `websocket`, `rest`, `multicast`
- `status` must be one of: `active`, `deprecated`, `disabled`
- At least one version required
- Version `effective_from` dates must not overlap
- Calendar `timezone` must be valid IANA timezone

### Schema Validation

- `name` must match filename (without `.yaml`)
- `schema_file` must exist and parse successfully
- `hash` must match SHA256 of schema file
- `status` must be one of: `draft`, `active`, `deprecated`
- At least one version required

### Environment Validation

- `name` must match filename (without `.yaml`)
- `feed` must reference existing feed in `feeds/`
- `schema` must reference existing schema with `active` status
- `transport.type` must be one of: `nats`, `mqtt`, `memory`
- `storage.type` must be one of: `local`, `s3`
- Required fields for each type must be present
