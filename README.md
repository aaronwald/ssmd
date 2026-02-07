# ssmd - Stupid Simple Market Data

Homelab-friendly market data capture, archival, and signal development.

## Exchanges

| Exchange | Type | Channels | Status |
|----------|------|----------|--------|
| **Kalshi** | Prediction markets | ticker, trade, market_lifecycle_v2 | Active |
| **Kraken** (spot) | Crypto spot | ticker, trade | Active |
| **Kraken** (futures) | Crypto perpetuals | ticker, trade | Code complete, pending deploy |
| **Polymarket** | Prediction markets (CLOB) | last_trade_price, price_change, book, best_bid_ask, new_market, market_resolved | Active |

## Components

| Component | Language | Purpose |
|-----------|----------|---------|
| **ssmd-connector** | Rust | WebSocket → NATS (trades, tickers, orderbook) |
| **ssmd-archiver** | Rust | NATS → JSONL.gz files (per-exchange) |
| **ssmd-cdc** | Rust | PostgreSQL CDC → NATS (dynamic subscriptions) |
| **ssmd-cache** | Rust | PostgreSQL + CDC → Redis cache |
| **ssmd-operator** | Go | K8s CRDs for pipeline topology |
| **ssmd** (CLI) | Deno | Metadata sync, backtesting, scaling, ops |
| **ssmd-data-ts** | Deno | HTTP API for secmaster + market data |
| **ssmd-signal-runner** | Deno | Real-time signal daemon |
| **ssmd-notifier** | Deno | Signal → ntfy.sh routing |
| **ssmd-lifecycle-consumer** | Deno | NATS → PostgreSQL (lifecycle events) |
| **ssmd-agent** | Deno | LangGraph REPL with market data tools |

## Quick Start

```bash
# Prerequisites: capnproto, rust, deno 2.x
make setup

# Build and test
make all
```

## CLI

```bash
cd ssmd-agent

# Secmaster sync (Kalshi, Kraken, Polymarket)
deno task cli secmaster sync --category Economics
deno task cli kraken sync
deno task cli polymarket sync

# Data quality
deno task cli dq trades --ticker KXBTCD-26FEB07-T98000
deno task cli dq trades --exchange kraken --ticker XXBTZUSD
deno task cli dq secmaster

# Scale operations
deno task cli scale status
deno task cli scale down --dry-run

# Signals
deno task cli signal list
deno task cli signal run volume-1m-30min

# Backtesting
deno task cli backtest run my-signal --from 2025-01-01 --to 2025-01-31

# Agent REPL
deno task agent
```

## Architecture

```
Kalshi WS ────── Connector ──┐
Kraken WS ────── Connector ──┤
Kraken Futures ── Connector ──┼──→ NATS JetStream ──┬──→ Archiver (JSONL.gz)
Polymarket WS ── Connector ──┘     (per-exchange     ├──→ Signal Runner
                                    streams)          └──→ Notifier (ntfy.sh)

PostgreSQL ←── secmaster sync (CLI/Temporal)
    │               │
    │          Kalshi, Kraken, Polymarket
    │          REST APIs (scheduled 6h)
    │
    ├──→ ssmd-cdc ──→ NATS (CDC stream) ──→ Connector (dynamic subs)
    │                                   └──→ ssmd-cache → Redis
    └──→ ssmd-data-ts (HTTP API)
              └──→ ssmd-agent (LangGraph REPL)
```

**Data flows:**
- **Market data**: Exchange WS → Connector → NATS → Archiver + Signal Runner
- **Lifecycle**: Kalshi WS → Lifecycle Connector → NATS → Consumer → PostgreSQL
- **CDC**: PostgreSQL → ssmd-cdc → NATS → Connector (dynamic market subscriptions)
- **Cache**: PostgreSQL + CDC stream → ssmd-cache → Redis
- **Secmaster**: Kalshi/Kraken/Polymarket REST APIs → CLI → PostgreSQL (via Temporal schedules)

## NATS Streams

| Stream | Subjects | Exchange | Retention |
|--------|----------|----------|-----------|
| PROD_KALSHI | `prod.kalshi.>` | Kalshi (all categories) | 512MB–1GB |
| PROD_KALSHI_POLITICS | `prod.kalshi.politics.>` | Kalshi (politics) | 512MB |
| PROD_KRAKEN | `prod.kraken.>` | Kraken spot | 256MB / 48h |
| PROD_KRAKEN_FUTURES | `prod.kraken-futures.>` | Kraken perpetuals | 256MB / 48h |
| PROD_POLYMARKET | `prod.polymarket.>` | Polymarket | 512MB / 48h |
| SECMASTER_CDC | `cdc.>` | CDC events (all tables) | 100MB / 15min |

## Connector Modules

Each exchange has its own module in `ssmd-rust/crates/connector/src/`:

| Module | Protocol | Endpoint |
|--------|----------|----------|
| `kalshi/` | Kalshi WS v1 | `wss://api.elections.kalshi.com/trade-api/ws/v2` |
| `kraken/` | Kraken WS v2 | `wss://ws.kraken.com/v2` |
| `kraken_futures/` | Kraken Futures WS v1 | `wss://futures.kraken.com/ws/v1` |
| `polymarket/` | Polymarket CLOB WS | `wss://ws-subscriptions-clob.polymarket.com/ws/market` |

Standard module structure: `mod.rs`, `messages.rs`, `websocket.rs`, `connector.rs`, `writer.rs`

## Secmaster

Multi-exchange market metadata stored in PostgreSQL:

| Exchange | Table | Records | Sync |
|----------|-------|---------|------|
| Kalshi | events, markets, series, series_fees | ~26k markets | 6h per-category Temporal schedules |
| Kraken | pairs (spot + perpetuals) | ~1,475 spot + ~32 perps | 6h Temporal schedule |
| Polymarket | polymarket_conditions, polymarket_tokens | ~3,200 conditions | 6h Temporal schedule |

Unified read-only `instruments` view spans all exchanges.

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `DATABASE_URL` | PostgreSQL connection |
| `NATS_URL` | NATS server (default: nats://localhost:4222) |
| `KALSHI_API_KEY` | Kalshi API key |
| `KALSHI_PRIVATE_KEY_PATH` | Path to RSA private key |
| `SSMD_API_URL` | ssmd-data-ts endpoint |
| `SSMD_DATA_API_KEY` | API key for ssmd-data-ts |
| `CDC_TABLES` | Tables for CDC publishing (default: events,markets,series_fees,pairs,polymarket_conditions,polymarket_tokens) |

## Documentation

| Doc | Purpose |
|-----|---------|
| [CLAUDE.md](CLAUDE.md) | Build commands, architecture |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Kubernetes deployment |
| [AGENT.md](AGENT.md) | Signal development agent |

## License

MIT
