# ssmd - Stupid Simple Market Data

Market data capture, archival, and signal development platform. Connects to exchange WebSocket APIs, publishes to NATS JetStream, and archives to compressed files.

## Supported Exchanges

| Exchange | Type | Channels |
|----------|------|----------|
| **Kalshi** | Prediction markets | ticker, trade, market_lifecycle_v2, orderbook_delta |
| **Kraken** (spot) | Crypto spot | ticker, trade |
| **Kraken** (futures) | Crypto perpetuals | ticker, trade |
| **Polymarket** | Prediction markets (CLOB) | last_trade_price, price_change, book, best_bid_ask, new_market, market_resolved |

## Architecture

![ssmd Architecture](docs/architecture.svg)

**Data flows:**
- **Market data**: Exchange WS → Connector → NATS → Archiver + Signal Runner + Consumers
- **Secmaster**: Exchange REST APIs → CLI → PostgreSQL → CDC → NATS
- **CDC**: PostgreSQL → ssmd-cdc → NATS → Connector (dynamic subs) + ssmd-cache (Redis)
- **Funding rates**: NATS → Funding Rate Consumer → pair_snapshots (PostgreSQL)
- **Signals**: Signal Runner → NATS (SIGNAL_FIRES) → Notifier → ntfy.sh
- **Archives**: Archiver → local PVC → GCS sync (Temporal scheduled)
- **Diagnosis**: PostgreSQL (scores) + data-ts (freshness/volume) → Claude → Email

## Components

### Rust (`ssmd-rust/`)

| Crate | Purpose |
|-------|---------|
| `ssmd-connector` | WebSocket → NATS publisher (multi-exchange) |
| `ssmd-archiver` | NATS → JSONL.gz file archiver (multi-stream) |
| `ssmd-cdc` | PostgreSQL logical replication → NATS |
| `ssmd-cache` | CDC stream → Redis cache |
| `middleware` | Transport, storage, and cache abstractions |
| `connector` (lib) | Exchange-specific WebSocket clients, writers, CDC consumer, shard manager |
| `schema` | Cap'n Proto message definitions |
| `metadata` | Feed and environment configuration |

### Deno/TypeScript (`ssmd-agent/`)

| Component | Path | Purpose |
|-----------|------|---------|
| CLI | `src/cli/` | Secmaster sync, data quality, scaling, fees, deployment |
| HTTP API | `src/server/` | REST API for market data and secmaster |
| Signal Runner | `src/runtime/` | Real-time signal evaluation daemon |
| Notifier | (standalone deploy) | Signal fire → ntfy.sh notification routing |
| Momentum | `src/momentum/` | Paper trading momentum engine + backtesting |
| Agent | `src/agent/` | LangGraph REPL with market data tools |
| Funding Rate Consumer | `src/cli/commands/funding-rate-consumer.ts` | NATS → pair_snapshots (Kraken Futures) |
| State Builders | `src/state/` | Orderbook, price history, volume profile |
| Shared Lib | `src/lib/` | DB (Drizzle), API clients, types (Zod), pricing, auth |

### Go (`ssmd-operators/`)

Kubernetes operator with CRDs for Connector, Archiver, Signal, and Notifier resources.

## Connector Modules

Each exchange has its own module in `ssmd-rust/crates/connector/src/`:

| Module | Protocol | Endpoint |
|--------|----------|----------|
| `kalshi/` | Kalshi WS | `wss://api.kalshi.com/trade-api/ws/v2` |
| `kraken/` | Kraken WS v2 | `wss://ws.kraken.com/v2` |
| `kraken_futures/` | Kraken Futures WS v1 | `wss://futures.kraken.com/ws/v1` |
| `polymarket/` | Polymarket CLOB WS | `wss://ws-subscriptions-clob.polymarket.com/ws/market` |

Common module structure: `mod.rs`, `messages.rs`, `websocket.rs`, `connector.rs`, `writer.rs`.

Exchange-specific extras:
- **Kalshi**: `auth.rs`, `cdc_consumer.rs`, `config.rs`, `shard_manager.rs` (CDC-driven dynamic subscriptions, sharding)
- **Polymarket**: `market_discovery.rs` (Gamma REST API discovery, secmaster-driven filtering)

Shared infrastructure in `connector/src/`: `nats_writer.rs`, `publisher.rs`, `metrics.rs`, `flusher.rs`, `ring_buffer.rs`, `secmaster.rs`.

## Building

```bash
# Prerequisites: capnproto, rust, deno 2.x
make setup

# Full validation (lint + security + test + build)
make all

# Rust only
make rust-all       # clippy + test + build
make rust-build     # build only
make rust-test      # test only

# TypeScript only
make agent-test     # run tests
make cli-check      # type check CLI
make agent-check    # type check CLI + agent
```

## NATS Subject Convention

```
{subject_prefix}.json.{type}.{ticker}
```

Where `subject_prefix` is typically `{env}.{feed}` or `{env}.{feed}.{category}` for sharded connectors.

Examples:
- `prod.kalshi.economics.json.trade.KXBTCD-26FEB07-T98000`
- `prod.kalshi.crypto.json.ticker.KXBTCD-26FEB07-T98000`
- `prod.kraken.json.ticker.XXBTZUSD`
- `prod.kraken-futures.json.trade.PF_XBTUSD`
- `prod.polymarket.json.last_trade_price.{condition_id}`

Message types vary by exchange:
- **Kalshi**: `ticker`, `trade`, `orderbook`, `lifecycle`, `event_lifecycle`
- **Kraken**: `ticker`, `trade`
- **Kraken Futures**: `ticker`, `trade`
- **Polymarket**: `last_trade_price`, `price_change`, `book`, `best_bid_ask`, `new_market`, `market_resolved`

## Agent Integration

Agents interact with ssmd through three interfaces: the HTTP API, LangGraph tools, and the CLI.

### HTTP API (`ssmd-data-ts`)

REST API with API key auth (`X-API-Key` header). Scopes: `secmaster:read`, `datasets:read`, `signals:read`, `signals:write`, `llm:chat`, `admin:read`, `admin:write`.

**Secmaster (all exchanges):**

| Endpoint | Description |
|----------|-------------|
| `GET /v1/events` | Kalshi events (filter: `category`, `status`, `series`, `as_of`) |
| `GET /v1/events/:ticker` | Event detail with markets |
| `GET /v1/markets` | Kalshi markets (filter: `category`, `status`, `series`, `close_within_hours`) |
| `GET /v1/markets/:ticker` | Market detail (prices, volume, open interest) |
| `GET /v1/secmaster/stats` | Unified stats across all exchanges |
| `GET /v1/secmaster/markets/timeseries` | Market activity timeseries (added/closed per day) |
| `GET /v1/secmaster/markets/active-by-category` | Active markets by category over time |
| `GET /v1/series` | Kalshi series (filter: `category`, `tag`, `games_only`) |
| `GET /v1/series/stats` | Series statistics |
| `GET /v1/pairs` | Kraken pairs (filter: `exchange`, `market_type`, `base`, `quote`) |
| `GET /v1/pairs/:pairId` | Pair detail (funding rate, mark price for perps) |
| `GET /v1/pairs/:pairId/snapshots` | Funding rate / price time series |
| `GET /v1/pairs/stats` | Pair statistics |
| `GET /v1/conditions` | Polymarket conditions (filter: `category`, `status`) |
| `GET /v1/conditions/:conditionId` | Condition with Yes/No tokens and prices |
| `GET /v1/polymarket/tokens` | Token IDs for connector subscriptions (filter: `category`, `minVolume`, `q`) |
| `GET /v1/fees` | All current fee schedules |
| `GET /v1/fees/:series` | Fee schedule for a Kalshi series |
| `GET /v1/fees/stats` | Fee statistics |

**Data & operations:**

| Endpoint | Description |
|----------|-------------|
| `GET /v1/markets/lookup` | Look up markets by ID across exchanges |
| `GET /datasets` | Archived datasets by feed/date |
| `GET /version` | API version |
| `GET /health` | Health check (no auth) |
| `GET /metrics` | Prometheus metrics (no auth) |
| `POST /v1/chat/completions` | OpenRouter LLM proxy with guardrails |

**Admin (requires `admin:read` or `admin:write`):**

| Endpoint | Description |
|----------|-------------|
| `POST /v1/keys` | Create API key |
| `GET /v1/keys` | List API keys |
| `PATCH /v1/keys/:prefix` | Update API key scopes |
| `DELETE /v1/keys/:prefix` | Revoke API key |
| `GET /v1/keys/usage` | Rate limit and token usage |
| `GET /v1/settings` | Get all settings |
| `PUT /v1/settings/:key` | Upsert setting |

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
| `get_fee_schedule` / `get_fees` | Fee lookup by series or tier |
| `list_datasets` / `sample_data` / `list_tickers` | Browse archived data |
| `get_schema` | Get schema for a message type |
| `orderbook_builder` / `price_history_builder` / `volume_profile_builder` | State builders for signal development |
| `list_builders` | List available state builders |
| `run_backtest` / `deploy_signal` | Signal evaluation and deployment |
| `get_today` | Current UTC date |

### CLI (`ssmd`)

| Command | Purpose |
|---------|---------|
| `secmaster sync/list/show/stats` | Kalshi secmaster sync and queries |
| `kraken` | Kraken spot + perpetuals sync |
| `polymarket` | Polymarket conditions sync |
| `fees sync/list/stats` | Fee schedule management |
| `series` | Series metadata operations |
| `health daily` | Pipeline health checks and email report |
| `diagnosis analyze` | AI-powered health/DQ analysis via Claude |
| `dq daily/trades` | Data quality scoring and trade checks |
| `keys create/list/update/revoke` | API key management |
| `share` | Generate signed URLs for parquet data sharing |
| `audit-email` | Daily data access audit report email |
| `scale down/up/status` | Cluster scaling for maintenance |
| `schedule list/describe` | Temporal schedule management |
| `archiver sync/deploy/list/status/logs/delete` | Archiver management |
| `connector deploy/list/status/logs/delete` | Connector CR management |
| `signal deploy/list/status/logs/delete` | Signal CR management |
| `notifier deploy/list/status/logs/delete` | Notifier CR management |
| `momentum run/backtest` | Momentum paper trading + backtesting |
| `funding-rate-consumer` | Kraken Futures funding rate NATS consumer |
| `status` | Cluster-wide status overview |
| `env` | Environment context management |
| `feed` | Feed configuration management |
| `init` | Initialize exchanges directory |
| `agent` | Start interactive LangGraph REPL |

## Documentation

| Doc | Purpose |
|-----|---------|
| [CLAUDE.md](CLAUDE.md) | Build commands, detailed architecture, latency design |
| [AGENT.md](AGENT.md) | Signal development agent |
| [Researcher Quickstart](docs/researcher-quickstart.md) | API access, MCP setup, data download |

### Data Schemas

Detailed field-level documentation for all exchange WebSocket messages and parquet output formats:

| Doc | Purpose |
|-----|---------|
| [Kalshi JSON Schema](docs/schemas/kalshi-json.md) | Ticker, trade, lifecycle WS messages — field definitions, sequence numbers, price units |
| [Kraken Futures JSON Schema](docs/schemas/kraken-futures-json.md) | V1 flat format, ticker/trade fields, funding rates, seq numbering |
| [Polymarket JSON Schema](docs/schemas/polymarket-json.md) | CLOB WS messages, condition/token identifier model, array fan-out |
| [Parquet Schemas](docs/schemas/parquet-schemas.md) | All 11 Arrow schemas, JSON→parquet column mapping, type conversions, versioning |

## License

MIT
