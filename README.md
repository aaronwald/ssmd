# ssmd - Stupid Simple Market Data

Market data capture, archival, and signal development platform. Connects to exchange WebSocket APIs, publishes to NATS JetStream, and archives to compressed files.

## Supported Exchanges

| Exchange | Type | Channels |
|----------|------|----------|
| **Kalshi** | Prediction markets | ticker, trade, market_lifecycle_v2 |
| **Kraken** (spot) | Crypto spot | ticker, trade |
| **Kraken** (futures) | Crypto perpetuals | ticker, trade |
| **Polymarket** | Prediction markets (CLOB) | last_trade_price, price_change, book, best_bid_ask, new_market, market_resolved |

## Architecture

```
Exchange WS ──→ Connector ──→ NATS JetStream ──→ Archiver (JSONL.gz)
                                    │
                                    ├──→ Signal Runner
                                    └──→ Notifier

PostgreSQL ←── secmaster sync (REST APIs)
    │
    ├──→ ssmd-cdc ──→ NATS (CDC) ──→ Connector (dynamic subs)
    └──→ ssmd-data-ts (HTTP API)
```

**Data flows:**
- **Market data**: Exchange WS → Connector → NATS → Archiver + Signal Runner
- **Secmaster**: Exchange REST APIs → CLI → PostgreSQL
- **CDC**: PostgreSQL → ssmd-cdc → NATS → Connector (dynamic market subscriptions)

## Components

### Rust (`ssmd-rust/`)

| Crate | Purpose |
|-------|---------|
| `ssmd-connector` | WebSocket → NATS publisher (multi-exchange) |
| `ssmd-archiver` | NATS → JSONL.gz file archiver |
| `ssmd-cdc` | PostgreSQL logical replication → NATS |
| `ssmd-cache` | CDC stream → Redis cache |
| `middleware` | Transport, storage, and cache abstractions |
| `connector` (lib) | Exchange-specific WebSocket clients and writers |
| `schema` | Cap'n Proto message definitions |
| `metadata` | Feed and environment configuration |

### Deno/TypeScript (`ssmd-agent/`)

| Component | Purpose |
|-----------|---------|
| CLI (`src/cli/`) | Secmaster sync, data quality, scaling, backtesting |
| HTTP API (`src/data/`) | REST API for market data and secmaster |
| Signal Runner | Real-time signal evaluation daemon |
| Notifier | Signal fire → notification routing |
| Agent (`src/agent/`) | LangGraph REPL with market data tools |

### Go (`ssmd-operators/`)

Kubernetes operator with CRDs for Connector, Archiver, Signal, and Notifier resources.

## Connector Modules

Each exchange has its own module in `ssmd-rust/crates/connector/src/`:

| Module | Protocol | Endpoint |
|--------|----------|----------|
| `kalshi/` | Kalshi WS | `wss://api.elections.kalshi.com/trade-api/ws/v2` |
| `kraken/` | Kraken WS v2 | `wss://ws.kraken.com/v2` |
| `kraken_futures/` | Kraken Futures WS v1 | `wss://futures.kraken.com/ws/v1` |
| `polymarket/` | Polymarket CLOB WS | `wss://ws-subscriptions-clob.polymarket.com/ws/market` |

Each module follows the same structure: `mod.rs`, `messages.rs`, `websocket.rs`, `connector.rs`, `writer.rs`.

## Building

```bash
# Prerequisites: capnproto, rust, deno 2.x
make setup

# Full validation (lint + test + build)
make all

# Rust only
make rust-all       # clippy + test + build

# TypeScript only
make agent-test     # run tests
make cli-check      # type check
```

## NATS Subject Convention

```
{env}.{feed}.json.{type}.{ticker}
```

Examples:
- `prod.kalshi.economics.json.trade.KXBTCD-26FEB07-T98000`
- `prod.kraken.json.ticker.XXBTZUSD`
- `prod.kraken-futures.json.trade.PF_XBTUSD`
- `prod.polymarket.json.last_trade_price.{condition_id}`

## Configuration

Connectors are configured via YAML feed and environment files in `exchanges/`:

```
exchanges/
├── feeds/           # Exchange WebSocket definitions
│   ├── kalshi.yaml
│   ├── kraken.yaml
│   ├── kraken-futures.yaml
│   └── polymarket.yaml
└── environments/    # Transport + storage per deployment
    ├── kalshi-prod.yaml
    ├── kraken-prod.yaml
    └── ...
```

## Agent Integration

Agents interact with ssmd through three interfaces: the HTTP API, LangGraph tools, and the CLI.

### HTTP API (`ssmd-data-ts`)

REST API with API key auth (`X-API-Key` header). Scopes: `secmaster:read`, `datasets:read`, `signals:read`, `llm:chat`, `admin:*`.

**Secmaster (all exchanges):**

| Endpoint | Description |
|----------|-------------|
| `GET /v1/events` | Kalshi events (filter: `category`, `status`, `series`, `as_of`) |
| `GET /v1/events/:ticker` | Event detail with markets |
| `GET /v1/markets` | Kalshi markets (filter: `category`, `status`, `series`, `close_within_hours`) |
| `GET /v1/markets/:ticker` | Market detail (prices, volume, open interest) |
| `GET /v1/series` | Kalshi series (filter: `category`, `tag`, `games_only`) |
| `GET /v1/pairs` | Kraken pairs (filter: `exchange`, `market_type`, `base`, `quote`) |
| `GET /v1/pairs/:pairId` | Pair detail (funding rate, mark price for perps) |
| `GET /v1/pairs/:pairId/snapshots` | Funding rate / price time series |
| `GET /v1/conditions` | Polymarket conditions (filter: `category`, `status`) |
| `GET /v1/conditions/:conditionId` | Condition with Yes/No tokens and prices |
| `GET /v1/polymarket/tokens` | Token IDs for connector subscriptions (filter: `category`, `minVolume`, `q`) |
| `GET /v1/fees/:series` | Fee schedule for a Kalshi series |
| `GET /v1/secmaster/stats` | Unified stats across all exchanges |

**Data & operations:**

| Endpoint | Description |
|----------|-------------|
| `GET /datasets` | Archived datasets by feed/date |
| `GET /health` | Health check (no auth) |
| `GET /metrics` | Prometheus metrics (no auth) |
| `POST /v1/chat/completions` | OpenRouter LLM proxy with guardrails |

### LangGraph Agent Tools

The agent REPL (`ssmd-agent/src/agent/`) provides LangGraph tools that wrap the API:

| Tool | Description |
|------|-------------|
| `list_markets` | Query Kalshi markets with point-in-time support |
| `get_market` | Get market by ticker |
| `list_events` / `get_event` | Kalshi events |
| `list_series` / `get_series` | Kalshi series |
| `list_pairs` / `get_pair` | Kraken trading pairs |
| `get_pair_snapshots` | Funding rate time series |
| `list_conditions` / `get_condition` | Polymarket conditions |
| `get_secmaster_stats` | Cross-exchange stats |
| `get_fee_schedule` | Fee lookup by series |
| `list_datasets` / `sample_data` | Browse archived data |
| `orderbook_builder` / `price_history_builder` / `volume_profile_builder` | State builders for signal development |
| `run_backtest` / `deploy_signal` | Signal evaluation and deployment |

### CLI (`ssmd`)

Key commands for agent/automation workflows:

```bash
# Secmaster
ssmd secmaster sync --category=Crypto --by-series
ssmd secmaster list --category=Crypto

# Data quality
ssmd dq daily --json          # JSON report for automation
ssmd dq trades --ticker KXBTCD-26FEB0317-T76999.99

# Scaling
ssmd scale down/up/status

# Archiver sync
ssmd archiver sync kalshi-archiver --wait
```

## Documentation

| Doc | Purpose |
|-----|---------|
| [CLAUDE.md](CLAUDE.md) | Build commands, detailed architecture |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Kubernetes deployment |
| [AGENT.md](AGENT.md) | Signal development agent |

## License

MIT
