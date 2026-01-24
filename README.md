# ssmd - Stupid Simple Market Data

Homelab-friendly market data capture, archival, and signal development.

## Components

| Component | Language | Purpose |
|-----------|----------|---------|
| **ssmd-connector** | Rust | WebSocket → NATS (trades, tickers, orderbook) |
| **ssmd-connector** (lifecycle) | Rust | WebSocket → NATS (market lifecycle events) |
| **ssmd-archiver** | Rust | NATS → JSONL.gz files |
| **ssmd-lifecycle-consumer** | Deno | NATS → PostgreSQL (lifecycle events) |
| **ssmd-cdc** | Rust | PostgreSQL CDC → NATS (for dynamic subs) |
| **ssmd-cache** | Rust | PostgreSQL + CDC → Redis cache |
| **ssmd** (CLI) | Deno | Metadata sync, backtesting, ops |
| **ssmd-data-ts** | Deno | HTTP API for data/secmaster |
| **ssmd-signal-runner** | Deno | Real-time signal daemon |
| **ssmd-notifier** | Deno | Signal → ntfy.sh routing |
| **ssmd-operator** | Go | K8s CRDs for pipeline |
| **ssmd-agent** | Deno | LangGraph REPL for signals |

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

# Secmaster sync
deno task cli secmaster sync --category Economics

# Signals
deno task cli signal list
deno task cli signal run volume-1m-30min

# Backtesting
deno task cli backtest run my-signal --from 2025-01-01 --to 2025-01-31

# Agent REPL
deno task agent
```

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `DATABASE_URL` | PostgreSQL connection |
| `NATS_URL` | NATS server (default: nats://localhost:4222) |
| `KALSHI_API_KEY` | Kalshi API key |
| `KALSHI_PRIVATE_KEY_PATH` | Path to RSA private key |
| `SSMD_API_URL` | ssmd-data-ts endpoint |
| `SSMD_DATA_API_KEY` | API key for ssmd-data-ts |

## Documentation

| Doc | Purpose |
|-----|---------|
| [CLAUDE.md](CLAUDE.md) | Build commands, architecture |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Kubernetes deployment |
| [AGENT.md](AGENT.md) | Signal development agent |

## Architecture

```
                         ┌───────────────────────────────────────────┐
                         │              NATS JetStream               │
                         └───────────────────────────────────────────┘
                            ↑              ↑              │         │
Kalshi WS ──┬── Connector ──┘              │              ↓         ↓
            │   (trades/tickers)           │        Archiver    Signal Runner
            │         ↑                    │            │            │
            │    (dynamic subs)            │            ↓            ↓
            │         │                    │        JSONL.gz      Notifier
            │         │                    │
            └── Lifecycle Connector ───────┘
                      │
                      ↓
              Lifecycle Consumer
                      │
                      ↓
                 PostgreSQL ←── secmaster sync (CLI/Temporal)
                      │
                      ├──── ssmd-cdc ──→ NATS (CDC stream) ──┬→ Connector
                      │                   (dynamic subs)     │  (dynamic subs)
                      │                                      ↓
                      │                                  ssmd-cache → Redis
                      ↓
                 ssmd-data-ts
                      ↓
               ssmd-agent (local)
```

**Data flows:**
- **Market data**: Kalshi WS → Connector → NATS → Archiver + Signal Runner
- **Lifecycle**: Kalshi WS → Lifecycle Connector → NATS → Consumer → PostgreSQL
- **CDC**: PostgreSQL → ssmd-cdc → NATS → Connector (dynamic market subscriptions)
- **Cache**: PostgreSQL (initial) + CDC stream → ssmd-cache → Redis

## Redis Cache Design

ssmd-cache maintains a Redis cache of secmaster data with intelligent TTL management.

**Key Structure:**
```
secmaster:series:{SERIES}                  # Series metadata
secmaster:series:{SERIES}:{EVENT}          # Event under series
secmaster:series:{SERIES}:{EVENT}:{MARKET} # Market under event
secmaster:fee:{SERIES}                     # Fee schedules
```

**TTL Policy:**
| Entity | Status | TTL |
|--------|--------|-----|
| Series | any | no expiry |
| Markets | active | no expiry |
| Markets | settled | +1 day from close_time |
| Events | active | no expiry |
| Events | non-active | +1 day from strike_date |

**Startup behavior:**
1. Capture snapshot LSN from PostgreSQL
2. Warm cache with active markets and events only
3. Subscribe to CDC stream, skip events before snapshot LSN
4. Apply CDC updates with TTL rules

## License

MIT
