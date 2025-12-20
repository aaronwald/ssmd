# ssmd - Stupid Simple Market Data

A git-native CLI for managing market data feed configuration.

## Quick Start

```bash
# Build the CLI
go build -o ssmd ./cmd/ssmd

# Initialize ssmd in your repository
./ssmd init
```

## Defining a Kalshi Feed

### 1. Create the feed

```bash
./ssmd feed create kalshi \
  --type websocket \
  --display-name "Kalshi Exchange" \
  --endpoint wss://trading-api.kalshi.com/trade-api/ws/v2 \
  --auth-method api_key
```

This creates `feeds/kalshi.yaml`.

### 2. Add capture locations

Track which datacenters will capture this feed:

```bash
./ssmd feed add-location kalshi --datacenter nyc1 --provider onprem
./ssmd feed add-location kalshi --datacenter aws-us-east-1 --provider aws --region us-east-1
```

### 3. View the feed configuration

```bash
./ssmd feed show kalshi
```

Output:
```
Name:         kalshi
Display Name: Kalshi Exchange
Type:         websocket
Status:       active

Current Version: v1 (effective 2025-12-20)
  Endpoint:    wss://trading-api.kalshi.com/trade-api/ws/v2
  Auth:        api_key
  Orderbook:   no
  Trades:      no

Capture Locations:
  nyc1 (onprem)
  aws-us-east-1 (aws, us-east-1)
```

### 4. Add a new version when the API changes

```bash
./ssmd feed add-version kalshi \
  --effective-from 2025-07-01 \
  --endpoint wss://trading-api.kalshi.com/trade-api/ws/v3
```

### 5. Validate and commit

```bash
./ssmd validate
./ssmd commit -m "Add Kalshi feed configuration"
```

## Resulting YAML

After the above commands, `feeds/kalshi.yaml` will contain:

```yaml
name: kalshi
display_name: Kalshi Exchange
type: websocket
status: active
capture_locations:
  - datacenter: nyc1
    provider: onprem
  - datacenter: aws-us-east-1
    provider: aws
    region: us-east-1
versions:
  - version: v1
    effective_from: "2025-12-20"
    protocol: wss
    endpoint: wss://trading-api.kalshi.com/trade-api/ws/v2
    auth_method: api_key
  - version: v2
    effective_from: "2025-07-01"
    protocol: wss
    endpoint: wss://trading-api.kalshi.com/trade-api/ws/v3
    auth_method: api_key
```

## Commands Reference

| Command | Description |
|---------|-------------|
| `ssmd init` | Initialize directory structure |
| `ssmd feed list` | List all feeds |
| `ssmd feed show <name>` | Show feed details |
| `ssmd feed create <name>` | Create a new feed |
| `ssmd feed update <name>` | Update feed properties |
| `ssmd feed add-version <name>` | Add a new version |
| `ssmd feed add-location <name>` | Add a capture location |
| `ssmd validate` | Validate all configuration |
| `ssmd diff` | Show uncommitted changes |
| `ssmd commit -m "msg"` | Commit changes to git |

See `ssmd <command> --help` for detailed flag options.
