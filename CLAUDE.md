# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ssmd - Simple/Streaming Market Data system

## Build Commands

All builds are done via Makefile:

```bash
# Full validation (lint + test + build)
make all

# Run all tests (Rust + TypeScript)
make test

# Run all lints (Rust clippy + Deno check)
make lint
```

### Rust Targets

```bash
make rust-build     # Build all Rust crates
make rust-test      # Run Rust tests
make rust-clippy    # Run Clippy linter
make rust-clean     # Clean Rust build artifacts
make rust-all       # Clippy + test + build
```

### TypeScript CLI/Agent (Deno)

```bash
make cli-check      # Type check CLI
make agent-check    # Type check CLI + agent
make agent-test     # Run agent tests
make agent-run      # Start agent REPL (requires ANTHROPIC_API_KEY)
```

### Database Migrations (dbmate)

Migrations are in `ssmd-agent/migrations/` and managed by [dbmate](https://github.com/amacneil/dbmate).

```bash
cd ssmd-agent

# Check migration status
deno task db:status

# Apply pending migrations
deno task db:migrate

# Rollback last migration
deno task db:rollback

# Create new migration
deno task db:new <name>
```

**Applying to Kubernetes environments:**

```bash
# 1. Port-forward to the target database
kubectl port-forward -n ssmd-dev svc/ssmd-postgres 5433:5432

# 2. Get credentials
kubectl get secret -n ssmd-dev ssmd-postgres-auth -o jsonpath='{.data.database-url}' | base64 -d

# 3. Run migrations (replace credentials)
DATABASE_URL="postgresql://ssmd:<password>@localhost:5433/ssmd?sslmode=disable" dbmate -d ./migrations --no-dump-schema up

# 4. Check status
DATABASE_URL="postgresql://ssmd:<password>@localhost:5433/ssmd?sslmode=disable" dbmate -d ./migrations status
```

**Migration file format:**

```sql
-- migrate:up
CREATE TABLE example (...);

-- migrate:down
DROP TABLE example;
```

## Prerequisites

```bash
# Install all dependencies (Debian/Ubuntu)
make setup
```

Or manually:

```bash
# System packages (required for Rust builds)
sudo apt-get install -y capnproto pkg-config libssl-dev

# Install Rust (if not present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Install Deno (if not present)
curl -fsSL https://deno.land/install.sh | sh
```

## Architecture

### Rust Crates (`ssmd-rust/`)

```
ssmd-rust/crates/
├── middleware/     # Pluggable abstractions (Transport, Storage, Cache, Journal)
├── connector/      # Market data collection runtime
├── schema/         # Cap'n Proto message definitions
├── metadata/       # Feed and environment configuration
└── ssmd-connector/ # Binary entrypoint
```

### Temporal Worker (`ssmd-worker/`)

```
ssmd-worker/       # Temporal worker (Node.js, shells out to ssmd CLI)
```

### TypeScript CLI/Agent (`ssmd-agent/`)

```
ssmd-agent/
├── src/cli/        # CLI commands (secmaster, backtest, fees, health, diagnosis)
├── src/lib/        # Shared library (db, api, types, guardrails)
├── src/server/     # data-ts HTTP API server (routes, auth, OpenRouter proxy)
├── src/agent/      # LangGraph agent with tools
├── prompts/        # System prompts (system.md)
└── src/main.ts     # Agent REPL entrypoint
```

### Diagnosis Pipeline

Daily AI-powered analysis of health and DQ results. Runs as a CronJob at 08:00 UTC.

```
┌─────────────┐     ┌───────────┐     ┌──────────┐     ┌─────────┐
│  PostgreSQL  │────▶│ diagnosis │────▶│ data-ts  │────▶│  Claude  │
│ dq_daily_   │     │  CLI pod  │     │ (proxy)  │     │ Sonnet   │
│ scores (7d) │     │           │◀────│          │◀────│ 4.6      │
└─────────────┘     │           │     └──────────┘     └─────────┘
                    │           │
┌─────────────┐     │           │     ┌──────────┐
│  data-ts    │────▶│           │────▶│  SMTP    │
│ /freshness  │     │           │     │  email   │
│ /volume     │     └───────────┘     └──────────┘
└─────────────┘
```

- **Input**: 7-day health scores (PostgreSQL), live freshness + volume (data-ts API)
- **AI**: Single Claude API call via data-ts OpenRouter proxy (`/v1/chat/completions`)
- **Output**: HTML email with status (GREEN/YELLOW/RED), per-feed diagnosis, trends, recommendations
- **System prompt**: Inlined in `src/cli/commands/diagnosis.ts` (compiled binary, no filesystem)
- **Model allowlist**: `src/lib/guardrails/mod.ts` — add models here to allow them through the proxy
- **Cost**: ~$0.03-0.05/day

### Latency Optimizations

The hot path is optimized to avoid syscalls and locks:

| Component | Implementation | Benefit |
|-----------|----------------|---------|
| Timestamps | `quanta` TSC clock | Zero syscalls (~10ns vs ~50ns) |
| Sequences | `AtomicU64` | Lock-free increment |
| Channel lookup | `DashMap` | Lock-free reads |
| String interning | `lasso` ThreadedRodeo | Avoid repeated allocations |
| File writes | SPSC ring buffer | Decouple hot path from disk I/O |

**Key modules:**
- `middleware/src/latency.rs` - TSC clock and string interner globals
- `connector/src/ring_buffer.rs` - SPSC mmap ring buffer (4MB, 1024 slots)
- `connector/src/flusher.rs` - Disk flusher with batching (runs on std::thread)

**Design principle:** Wall-clock timestamps only at disk boundary (syscall OK when doing I/O anyway).

## Running the Connector

```bash
# Build first
make rust-build

# Run Kalshi connector (requires feed/env YAML files, KALSHI_API_KEY, KALSHI_PRIVATE_KEY, and NATS)
./ssmd-rust/target/debug/ssmd-connector \
  --feed /path/to/kalshi.yaml \
  --env /path/to/kalshi-local.yaml

# For demo API, also set KALSHI_USE_DEMO=true
```

The `--feed` and `--env` arguments are **file paths** to YAML configuration files (managed privately, not in this repo).

## Kubernetes Operator

The ssmd-operators project manages Kubernetes CRDs for market data components:

| CRD | Purpose |
|-----|---------|
| `connectors.ssmd.ssmd.io` | Manages WebSocket connector pods |
| `archivers.ssmd.ssmd.io` | Manages NATS → JSONL.gz archiver pods |
| `signals.ssmd.ssmd.io` | Manages signal evaluation pods |
| `notifiers.ssmd.ssmd.io` | Manages alert notification pods |

```bash
# Generate CRDs after modifying *_types.go
cd ssmd-operators && make manifests

# Build locally
make build
```

## Instructions

1. All code must go through pr code review.
2. Follow the build instructions in CLAUDE.md. Limit freelancing unless we are in a brainstorming session.
