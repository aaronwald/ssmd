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

## Documentation

| Doc | Purpose |
|-----|---------|
| [CLAUDE.md](CLAUDE.md) | Build commands, detailed architecture |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Kubernetes deployment |
| [AGENT.md](AGENT.md) | Signal development agent |

## License

MIT
