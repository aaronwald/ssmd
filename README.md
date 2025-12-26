# ssmd - Stupid Simple Market Data

A homelab-friendly market data system with GitOps configuration. Capture, archive, and analyze market data with an AI-powered signal development workflow.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              ssmd System                                 │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐               │
│  │   Connector  │───▶│     NATS     │───▶│   Archiver   │               │
│  │    (Rust)    │    │  JetStream   │    │    (Rust)    │               │
│  └──────────────┘    └──────────────┘    └──────────────┘               │
│         │                   │                   │                        │
│         │                   │                   ▼                        │
│         │                   │           ┌──────────────┐                │
│         │                   │           │  JSONL.gz    │                │
│         │                   │           │  + Manifest  │                │
│         │                   │           └──────────────┘                │
│         │                   │                   │                        │
│         │                   │                   ▼                        │
│         │                   │           ┌──────────────┐                │
│         │                   │           │  ssmd-data   │                │
│         │                   │           │   (Go API)   │                │
│         │                   │           └──────────────┘                │
│         │                   │                   │                        │
│         │                   │                   ▼                        │
│         │                   │           ┌──────────────┐                │
│         │                   │           │  ssmd-agent  │                │
│         │                   │           │ (Deno REPL)  │                │
│         │                   │           └──────────────┘                │
│         │                   │                                            │
│         ▼                   ▼                                            │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                    ssmd CLI (Go)                                 │    │
│  │  Metadata management: feeds, schemas, environments, keys        │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## Components

| Component | Language | Purpose |
|-----------|----------|---------|
| **ssmd** | Go | CLI for GitOps metadata management |
| **ssmd-connector** | Rust | WebSocket client → NATS publisher |
| **ssmd-archiver** | Rust | NATS subscriber → JSONL.gz files |
| **ssmd-data** | Go | HTTP API for querying archived data |
| **ssmd-agent** | Deno/TS | LangGraph REPL for signal development |

## Quick Start

### Prerequisites

```bash
# Install Cap'n Proto (required for Rust builds)
sudo apt-get install -y capnproto

# Install Rust (if not present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

### Build Everything

```bash
# Build and test all components
make all
```

### Directory Structure

```
.
├── cmd/
│   └── ssmd-data/         # Data API server
├── internal/              # Go packages
│   ├── api/               # HTTP handlers
│   ├── cmd/               # CLI commands
│   ├── data/              # Storage abstraction
│   └── types/             # Shared types
├── ssmd-rust/
│   └── crates/
│       ├── connector/     # Market data connector
│       ├── middleware/    # Transport, Storage, Cache, Journal traits
│       ├── schema/        # Cap'n Proto definitions
│       ├── metadata/      # Feed/Environment config
│       ├── ssmd-connector/# Connector binary
│       └── ssmd-archiver/ # Archiver binary
├── ssmd-agent/            # LangGraph agent
│   ├── src/
│   │   ├── tools/         # Agent tools
│   │   ├── skills/        # Markdown skill templates
│   │   └── builders/      # State builders (OrderBook)
│   └── Dockerfile
├── exchanges/             # Feed configurations
│   ├── feeds/             # Feed YAML files
│   ├── schemas/           # Schema definitions
│   └── environments/      # Environment configs
├── docs/
│   ├── plans/             # Implementation plans
│   └── reference/         # CLI reference docs
└── implementation/        # Daily implementation notes
```

## Build Commands

```bash
# Everything (lint + test + build for Go and Rust)
make all

# Go only
make build          # Build CLI
make test           # Run tests
make lint           # Run vet + staticcheck

# Rust only
make rust-build     # Build all Rust crates
make rust-test      # Run Rust tests
make rust-clippy    # Run Clippy

# Data API
make data-build     # Build ssmd-data
make data-test      # Test API handlers
make data-run       # Run with test data

# Agent
make agent-check    # Deno check
make agent-test     # Run agent tests
make agent-run      # Start REPL
```

## Running the Connector

The connector captures market data from configured feeds and publishes to NATS.

```bash
# Build
make rust-build

# Set credentials
export KALSHI_API_KEY="your-api-key"
export KALSHI_PRIVATE_KEY="$(cat ~/.kalshi/private.pem)"
export KALSHI_USE_DEMO=true  # For demo API

# Run
./ssmd-rust/target/debug/ssmd-connector \
  --feed ./exchanges/feeds/kalshi.yaml \
  --env ./exchanges/environments/kalshi-local.yaml
```

The connector requires NATS. Configure transport in environment YAML:

```yaml
transport:
  transport_type: nats
  url: nats://localhost:4222
```

## Running the Archiver

The archiver subscribes to NATS and writes JSONL.gz files with manifests.

```bash
./ssmd-rust/target/debug/ssmd-archiver \
  --env ./exchanges/environments/kalshi-local.yaml \
  --output /data/kalshi
```

Output structure:
```
/data/kalshi/2025-12-25/
├── trades_000.jsonl.gz
├── tickers_000.jsonl.gz
└── manifest.json
```

## Running the Data API

```bash
# With test data
make data-run

# Or manually
SSMD_DATA_PATH=/data SSMD_API_KEY=mykey ./bin/ssmd-data
```

### API Endpoints

```bash
# List datasets
curl -H "X-API-Key: dev" http://localhost:8080/datasets

# Filter by feed/date
curl -H "X-API-Key: dev" "http://localhost:8080/datasets?feed=kalshi&from=2025-12-20"

# Sample records
curl -H "X-API-Key: dev" "http://localhost:8080/datasets/kalshi/2025-12-25/sample?limit=10"

# Get schema
curl -H "X-API-Key: dev" http://localhost:8080/schema/kalshi/trade

# List state builders
curl -H "X-API-Key: dev" http://localhost:8080/builders
```

## Running the Agent

The agent is a LangGraph-powered CLI for developing trading signals.

```bash
# Set API keys
export ANTHROPIC_API_KEY="your-key"
export SSMD_DATA_URL="http://localhost:8080"
export SSMD_API_KEY="dev"

# Run REPL
make agent-run
```

Example session:
```
> explore kalshi data from yesterday
[Lists datasets, samples trades]

> create a spread monitor for INXD that fires when spread > 5
[Generates TypeScript signal, runs backtest]

> deploy it
[Writes signal to signals/ directory]
```

## CLI Reference

### Metadata Commands

```bash
# Initialize
ssmd init

# Feeds
ssmd feed list
ssmd feed show kalshi
ssmd feed create kalshi --type websocket --endpoint wss://...

# Schemas
ssmd schema list
ssmd schema register orderbook --file schemas/orderbook.capnp

# Environments
ssmd env list
ssmd env show kalshi-prod
ssmd env create kalshi-prod --feed kalshi --transport.type nats

# Keys
ssmd key list kalshi-prod
ssmd key verify kalshi-prod

# Validation
ssmd validate
ssmd validate --check-keys
```

### Data Commands

```bash
# List datasets
ssmd data list --feed kalshi --from 2025-12-20

# Sample records
ssmd data sample kalshi 2025-12-25 --ticker INXD --limit 10

# Show schema
ssmd data schema kalshi trade

# List builders
ssmd data builders
```

## Container Images

All images published to GitHub Container Registry:

| Image | Description |
|-------|-------------|
| `ghcr.io/aaronwald/ssmd-connector` | Market data connector |
| `ghcr.io/aaronwald/ssmd-archiver` | NATS → file archiver |
| `ghcr.io/aaronwald/ssmd-data` | Dataset HTTP API |
| `ghcr.io/aaronwald/ssmd-agent` | LangGraph agent |

```bash
# Pull latest
docker pull ghcr.io/aaronwald/ssmd-connector:0.2.0
docker pull ghcr.io/aaronwald/ssmd-archiver:0.2.0
docker pull ghcr.io/aaronwald/ssmd-data:0.2.0
docker pull ghcr.io/aaronwald/ssmd-agent:0.2.0
```

## Development Workflow

### Daily Workflow

1. Read `TODO.md` for current priorities
2. Check `implementation/` for recent session notes
3. Review `docs/plans/` for design context

### Making Changes

```bash
# Create feature branch
git checkout -b feature/my-feature

# Make changes, run tests
make all

# Commit (Claude Code will add co-author)
git add -A
git commit -m "feat: description"

# Push and create PR
git push -u origin feature/my-feature
gh pr create
```

### Tagging Releases

```bash
git tag -a v0.2.1 -m "description"
git push origin v0.2.1
# GitHub Actions builds and publishes containers
```

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Go for CLI | `ssmd` | Fast builds, single binary, Cobra ecosystem |
| Rust for hot path | Connector, archiver | Zero-cost abstractions, memory safety |
| NATS JetStream | Message transport | Persistence, replay, simple ops |
| Cap'n Proto | Wire format | Zero-copy, schema evolution |
| Deno for agent | TypeScript runtime | Native TS, secure by default |
| LangGraph | Agent framework | Stateful, tool-calling, streaming |
| JSONL.gz | Archive format | Grep-friendly, compressed |
| GitOps | Config management | Versioned, reviewable, auditable |

## Latency Optimizations (Connector)

The hot path avoids syscalls and locks:

| Component | Implementation | Benefit |
|-----------|----------------|---------|
| Timestamps | `quanta` TSC | ~10ns vs ~50ns syscall |
| Sequences | `AtomicU64` | Lock-free |
| Channel lookup | `DashMap` | Lock-free reads |
| String interning | `lasso` | Avoid allocations |
| File writes | SPSC ring buffer | Decouple from disk I/O |

## Documentation

- `CLAUDE.md` - Build commands and architecture notes
- `TODO.md` - Task tracking and roadmap
- `docs/plans/` - Design documents and implementation plans
- `docs/reference/` - CLI reference, file formats, GitOps overview
- `implementation/` - Daily session notes

## License

MIT
