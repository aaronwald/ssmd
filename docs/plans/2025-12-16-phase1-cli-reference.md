# CLI Reference

> Command reference for the `ssmd` CLI tool.

## Global Flags

| Flag | Description |
|------|-------------|
| `--help`, `-h` | Show help for command |
| `--quiet`, `-q` | Suppress non-error output |
| `--verbose`, `-v` | Show detailed output |

---

## Feed Commands

### ssmd feed list

List all registered feeds.

```bash
ssmd feed list
ssmd feed list --status active
```

| Flag | Description |
|------|-------------|
| `--status` | Filter by status: `active`, `deprecated`, `disabled` |

**Output:**
```
NAME          TYPE        STATUS    VERSIONS
kalshi        websocket   active    2
polymarket    websocket   active    1
```

### ssmd feed show

Show details for a specific feed.

```bash
ssmd feed show kalshi
ssmd feed show kalshi --version v1
```

| Flag | Description |
|------|-------------|
| `--version` | Show specific version details |

**Output:**
```
Name:         kalshi
Display Name: Kalshi Exchange
Type:         websocket
Status:       active

Current Version: v2 (effective 2025-01-01)
  Endpoint:    wss://api.kalshi.com/trade-api/ws/v2
  Auth:        api_key
  Rate Limit:  10/sec
  Orderbook:   yes
  Trades:      yes

Calendar:
  Timezone:    America/New_York
  Hours:       04:00 - 00:00
```

### ssmd feed create

Create a new feed.

```bash
ssmd feed create kalshi --type websocket
ssmd feed create kalshi \
  --type websocket \
  --display-name "Kalshi Exchange" \
  --endpoint wss://api.kalshi.com/trade-api/ws/v2 \
  --auth-method api_key
```

| Flag | Description |
|------|-------------|
| `--type` | Feed type: `websocket`, `rest`, `multicast` (required) |
| `--display-name` | Human-readable name |
| `--endpoint` | Connection URL |
| `--auth-method` | Authentication: `api_key`, `oauth`, `mtls`, `none` |
| `--rate-limit` | Requests per second |
| `--supports-orderbook` | Feed provides orderbook data |
| `--supports-trades` | Feed provides trade data |
| `--effective-from` | Version effective date (default: today) |

Creates `feeds/<name>.yaml`.

### ssmd feed update

Update an existing feed.

```bash
ssmd feed update kalshi --rate-limit 15
ssmd feed update kalshi --version v1 --rate-limit 5
```

| Flag | Description |
|------|-------------|
| `--version` | Target specific version (default: latest) |
| `--display-name` | Update display name |
| `--endpoint` | Update endpoint |
| `--rate-limit` | Update rate limit |
| `--status` | Update status |
| (other flags) | Same as create |

### ssmd feed add-version

Add a new version to a feed.

```bash
ssmd feed add-version kalshi --effective-from 2025-07-01
ssmd feed add-version kalshi \
  --effective-from 2025-07-01 \
  --endpoint wss://api.kalshi.com/v3
```

| Flag | Description |
|------|-------------|
| `--effective-from` | When version takes effect (required) |
| `--copy-from` | Copy settings from version (default: latest) |
| (other flags) | Override specific fields |

---

## Schema Commands

### ssmd schema list

List all registered schemas.

```bash
ssmd schema list
ssmd schema list --status active
```

| Flag | Description |
|------|-------------|
| `--status` | Filter by status: `draft`, `active`, `deprecated` |

**Output:**
```
NAME        VERSION   FORMAT   STATUS      EFFECTIVE
trade       v1        capnp    active      2025-01-01
trade       v2        capnp    draft       2025-06-01
orderbook   v1        capnp    active      2025-01-01
```

### ssmd schema show

Show details for a specific schema.

```bash
ssmd schema show trade
ssmd schema show trade:v1
```

**Output:**
```
Name:    trade
Format:  capnp
File:    schemas/trade.capnp

Versions:
  v1 (active, 2025-01-01)
    Hash: sha256:a1b2c3d4e5f6...

  v2 (draft, 2025-06-01)
    Hash: sha256:f6e5d4c3b2a1...
    Compatible with: v1
    Breaking changes: Added takerSide field
```

### ssmd schema register

Register a new schema.

```bash
ssmd schema register trade --file schemas/trade.capnp
ssmd schema register trade \
  --file schemas/trade.capnp \
  --format capnp \
  --status draft
```

| Flag | Description |
|------|-------------|
| `--file` | Path to schema definition file (required) |
| `--format` | Schema format: `capnp`, `protobuf`, `json_schema` (default: inferred) |
| `--status` | Initial status: `draft`, `active` (default: `active`) |
| `--effective-from` | Version effective date (default: today) |

Creates `schemas/<name>.yaml` and copies definition file if not already in `schemas/`.

### ssmd schema hash

Recompute hash for a schema.

```bash
ssmd schema hash trade
ssmd schema hash --all
```

| Flag | Description |
|------|-------------|
| `--all` | Recompute hashes for all schemas |

Updates the hash in the metadata file if changed.

### ssmd schema set-status

Change schema version status.

```bash
ssmd schema set-status trade:v1 deprecated
ssmd schema set-status trade:v2 active
```

**Arguments:**
1. Schema reference as `name:version`
2. New status: `draft`, `active`, `deprecated`

### ssmd schema add-version

Add a new version to a schema.

```bash
ssmd schema add-version trade \
  --file schemas/trade-v2.capnp \
  --effective-from 2025-06-01 \
  --compatible-with v1
```

| Flag | Description |
|------|-------------|
| `--file` | Path to new schema definition (required) |
| `--effective-from` | When version takes effect (required) |
| `--status` | Initial status (default: `draft`) |
| `--compatible-with` | Comma-separated list of compatible versions |
| `--breaking-changes` | Description of breaking changes |

---

## Environment Commands

### ssmd env list

List all environments.

```bash
ssmd env list
```

**Output:**
```
NAME          FEED      SCHEMA     TRANSPORT
kalshi-dev    kalshi    trade:v1   nats
kalshi-prod   kalshi    trade:v1   nats
```

### ssmd env show

Show details for an environment.

```bash
ssmd env show kalshi-dev
```

**Output:**
```
Name:     kalshi-dev
Feed:     kalshi
Schema:   trade:v1

Schedule:
  Timezone:  UTC
  Start:     00:10
  End:       00:00
  Auto-roll: yes

Keys:
  kalshi (api_key, required)
    Fields: api_key, api_secret
    Source: env

Transport:
  Type: nats
  URL:  nats://localhost:4222

Storage:
  Type: local
  Path: /var/lib/ssmd/data
```

### ssmd env create

Create a new environment.

```bash
ssmd env create kalshi-dev --feed kalshi --schema trade:v1
ssmd env create kalshi-dev \
  --feed kalshi \
  --schema trade:v1 \
  --transport.type nats \
  --transport.url nats://localhost:4222 \
  --storage.type local \
  --storage.path /var/lib/ssmd/data
```

| Flag | Description |
|------|-------------|
| `--feed` | Feed reference (required) |
| `--schema` | Schema reference as `name:version` (required) |
| `--schedule.timezone` | Schedule timezone |
| `--schedule.day-start` | Collection start time |
| `--schedule.day-end` | Collection end time |
| `--transport.type` | Transport type: `nats`, `mqtt`, `memory` |
| `--transport.url` | Transport URL |
| `--storage.type` | Storage type: `local`, `s3` |
| `--storage.path` | Local storage path |
| `--storage.bucket` | S3 bucket name |
| `--storage.region` | S3 region |

### ssmd env update

Update an existing environment.

```bash
ssmd env update kalshi-dev --transport.url nats://newhost:4222
ssmd env update kalshi-dev --schema trade:v2
```

Flags same as create. Only specified fields are updated.

### ssmd env add-key

Add a key reference to an environment.

```bash
ssmd env add-key kalshi-dev kalshi \
  --type api_key \
  --fields api_key,api_secret \
  --source env
```

| Flag | Description |
|------|-------------|
| `--type` | Key type: `api_key`, `database`, `transport`, `storage` |
| `--fields` | Comma-separated list of field names |
| `--source` | Source: `env`, `sealed-secret/<name>`, `vault/<path>` |
| `--required` | Whether key is required (default: `true`) |

---

## Validation Commands

### ssmd validate

Validate configuration files.

```bash
ssmd validate
ssmd validate feeds/kalshi.yaml
ssmd validate environments/
```

**Arguments:**
- No arguments: validate all files
- File path: validate specific file
- Directory: validate all files in directory

**Output:**
```
feeds/kalshi.yaml                    ✓ valid
feeds/polymarket.yaml                ✓ valid
schemas/trade.yaml                   ✓ valid (hash matches)
schemas/orderbook.yaml               ✗ hash mismatch
environments/kalshi-dev.yaml         ✓ valid
environments/kalshi-prod.yaml        ✗ references schema 'quote:v1' not found

Errors: 2
Warnings: 0
```

Exit code: 0 if valid, 1 if errors.

---

## Git Commands

### ssmd diff

Show uncommitted changes to ssmd files.

```bash
ssmd diff
```

**Output:**
```
Modified:
  feeds/kalshi.yaml

New:
  feeds/polymarket.yaml
  schemas/orderbook.capnp
  schemas/orderbook.yaml

Deleted:
  environments/test.yaml
```

### ssmd commit

Commit changes to git.

```bash
ssmd commit -m "Add Kalshi feed"
ssmd commit -m "Update rate limits" --no-validate
```

| Flag | Description |
|------|-------------|
| `-m`, `--message` | Commit message (required) |
| `--no-validate` | Skip validation before commit |

**Behavior:**
1. Run `ssmd validate` (unless `--no-validate`)
2. Fail if validation errors
3. `git add` all modified ssmd files (feeds/, schemas/, environments/)
4. `git commit -m "<message>"`

Does NOT push.

---

## Init Command

### ssmd init

Initialize ssmd in a repository.

```bash
ssmd init
```

Creates directory structure:
```
feeds/
schemas/
environments/
.ssmd/
  config.yaml
```

Adds `.ssmd/` to `.gitignore`.
