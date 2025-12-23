# ssmd - Stupid Simple Market Data

A git-native CLI for managing market data feed configuration. All configuration is stored as YAML files, versioned with git.

## Quick Start

```bash
# Build the CLI
make build

# Initialize ssmd in your repository
./ssmd init

# This creates:
#   feeds/         - Feed configurations
#   schemas/       - Schema definitions
#   environments/  - Environment configs
#   .ssmd/         - Local CLI state (gitignored)
```

## Feeds

Feeds define data sources (WebSocket, REST, multicast).

### Create a feed

```bash
./ssmd feed create kalshi \
  --type websocket \
  --display-name "Kalshi Exchange" \
  --endpoint wss://trading-api.kalshi.com/trade-api/ws/v2 \
  --auth-method api_key
```

### List and show feeds

```bash
./ssmd feed list
./ssmd feed show kalshi
```

### Add capture locations

Track which datacenters capture this feed (for provenance):

```bash
./ssmd feed add-location kalshi --datacenter nyc1 --provider onprem
./ssmd feed add-location kalshi --datacenter aws-us-east-1 --provider aws --region us-east-1
```

### Add a new version

When the API changes, add a new version:

```bash
./ssmd feed add-version kalshi \
  --effective-from 2025-07-01 \
  --endpoint wss://trading-api.kalshi.com/trade-api/ws/v3
```

Use `--effective-to` for explicit date ranges:

```bash
./ssmd feed add-version kalshi \
  --effective-from 2025-01-01 \
  --effective-to 2025-06-30 \
  --endpoint wss://trading-api.kalshi.com/trade-api/ws/v2
```

### Update feed properties

```bash
./ssmd feed update kalshi --display-name "Kalshi Prediction Market"
```

## Schemas

Schemas define the structure of normalized data (Cap'n Proto, Protobuf, JSON Schema).

### Register a schema

```bash
./ssmd schema register orderbook \
  --file schemas/orderbook.capnp \
  --format capnp \
  --effective-from 2025-01-01
```

### List and show schemas

```bash
./ssmd schema list
./ssmd schema show orderbook
```

### Add a new version

```bash
./ssmd schema add-version orderbook \
  --file schemas/orderbook_v2.capnp \
  --effective-from 2025-07-01
```

### Recompute hash

After editing a schema file:

```bash
./ssmd schema hash orderbook
```

### Change version status

```bash
./ssmd schema set-status orderbook v1 deprecated
./ssmd schema set-status orderbook v2 active
```

Status options: `draft`, `active`, `deprecated`

## Environments

Environments tie together a feed, schema, transport, and storage configuration.

### Create an environment

```bash
./ssmd env create kalshi-prod \
  --feed kalshi \
  --schema orderbook:v1 \
  --transport.type nats \
  --transport.url nats://localhost:4222 \
  --storage.type s3 \
  --storage.bucket market-data \
  --storage.region us-east-1 \
  --schedule.timezone America/New_York \
  --schedule.day-start "09:00" \
  --schedule.day-end "17:00"
```

### List and show environments

```bash
./ssmd env list
./ssmd env show kalshi-prod
```

### Add key references

Reference secrets stored in your secrets manager:

```bash
./ssmd env add-key kalshi-prod \
  --name api_key \
  --provider vault \
  --path secret/kalshi/api_key

./ssmd env add-key kalshi-prod \
  --name api_secret \
  --provider sealed_secret \
  --path kalshi-credentials
```

### Update environment

```bash
./ssmd env update kalshi-prod --storage.bucket new-bucket-name
```

## Git Workflow

ssmd integrates with git for version control.

### Validate configuration

Check all files for correctness and referential integrity:

```bash
./ssmd validate
```

Validate a specific file or directory:

```bash
./ssmd validate feeds/kalshi.yaml
./ssmd validate environments/
```

### View changes

```bash
./ssmd diff
```

### Commit changes

```bash
./ssmd commit -m "Add Kalshi feed configuration"
```

## Directory Structure

```
.
├── feeds/
│   └── kalshi.yaml
├── schemas/
│   ├── orderbook.yaml       # Schema metadata
│   └── orderbook.capnp      # Schema definition
├── environments/
│   └── kalshi-prod.yaml
└── .ssmd/                   # Local state (gitignored)
```

## Example: Complete Kalshi Setup

```bash
# Initialize
./ssmd init

# Create feed
./ssmd feed create kalshi \
  --type websocket \
  --display-name "Kalshi Exchange" \
  --endpoint wss://trading-api.kalshi.com/trade-api/ws/v2 \
  --auth-method api_key

# Add capture location
./ssmd feed add-location kalshi --datacenter nyc1 --provider onprem

# Register schema
./ssmd schema register kalshi-events \
  --file schemas/kalshi-events.capnp \
  --format capnp

# Create environment
./ssmd env create kalshi-prod \
  --feed kalshi \
  --schema kalshi-events:v1 \
  --transport.type nats \
  --transport.url nats://localhost:4222 \
  --storage.type local \
  --storage.path /data/kalshi

# Add API key reference
./ssmd env add-key kalshi-prod \
  --name api_key \
  --provider env \
  --path KALSHI_API_KEY

# Validate and commit
./ssmd validate
./ssmd commit -m "Add Kalshi feed configuration"
```

## Command Reference

### Root Commands

| Command | Description |
|---------|-------------|
| `ssmd init` | Initialize directory structure |
| `ssmd validate [path]` | Validate configuration files |
| `ssmd diff` | Show uncommitted changes |
| `ssmd commit -m "msg"` | Commit changes to git |

### Feed Commands

| Command | Description |
|---------|-------------|
| `ssmd feed list` | List all feeds |
| `ssmd feed show <name>` | Show feed details |
| `ssmd feed create <name>` | Create a new feed |
| `ssmd feed update <name>` | Update feed properties |
| `ssmd feed add-version <name>` | Add a new version |
| `ssmd feed add-location <name>` | Add a capture location |

### Schema Commands

| Command | Description |
|---------|-------------|
| `ssmd schema list` | List all schemas |
| `ssmd schema show <name>` | Show schema details |
| `ssmd schema register <name>` | Register a new schema |
| `ssmd schema add-version <name>` | Add a new version |
| `ssmd schema hash <name>` | Recompute schema hash |
| `ssmd schema set-status <name> <version> <status>` | Change version status |

### Environment Commands

| Command | Description |
|---------|-------------|
| `ssmd env list` | List all environments |
| `ssmd env show <name>` | Show environment details |
| `ssmd env create <name>` | Create a new environment |
| `ssmd env update <name>` | Update environment properties |
| `ssmd env add-key <name>` | Add a key reference |

## Development

```bash
make build       # Build Go CLI
make test        # Run Go tests
make lint        # Run vet + staticcheck
make clean       # Remove binary
make tools       # Install staticcheck
make all         # lint + test + build (Go + Rust)

# Rust-specific
make rust-build  # Build Rust crates
make rust-test   # Run Rust tests
make rust-clippy # Run Rust linter
```

## Running the Connector

The Rust connector captures market data from configured feeds.

```bash
# Build the connector
make rust-build

# Create data directory
mkdir -p ./data

# Run Kalshi connector
./ssmd-rust/target/debug/ssmd-connector \
  --feed ./exchanges/feeds/kalshi.yaml \
  --env ./exchanges/environments/kalshi-local.yaml
```

**Required environment variables for Kalshi:**
- `KALSHI_API_KEY` - Your Kalshi API key
- `KALSHI_PRIVATE_KEY` - Your RSA private key (PEM format)
- `KALSHI_USE_DEMO` - Set to `true` for demo API (optional)

The `--feed` and `--env` arguments are **file paths** to YAML configuration files, not feed/environment names.
