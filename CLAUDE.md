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

### TypeScript CLI/Agent (`ssmd-agent/`)

```
ssmd-agent/
├── src/cli/        # CLI commands (secmaster, backtest, fees, data)
├── src/lib/        # Shared library (db, api, types)
├── src/agent/      # LangGraph agent with tools
└── src/main.ts     # Agent REPL entrypoint
```

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

# Run Kalshi connector (requires KALSHI_API_KEY, KALSHI_PRIVATE_KEY, and NATS)
./ssmd-rust/target/debug/ssmd-connector \
  --feed ./exchanges/feeds/kalshi.yaml \
  --env ./exchanges/environments/kalshi-local.yaml

# For demo API, also set KALSHI_USE_DEMO=true
```

The connector requires NATS transport. Configure in environment YAML:
```yaml
transport:
  transport_type: nats
  url: nats://localhost:4222
```

The `--feed` and `--env` arguments are **file paths**, not names.

## Docker Images

All images are built via GitHub Actions - **do not use docker/podman directly**.

### Build Triggers

Images build automatically on git tag push, or via manual workflow dispatch.

### Triggering a Build

```bash
# Option 1: Tag and push (use the correct tag format per workflow)
git tag v0.4.4           # Rust connector/archiver (triggers: v*)
git tag cli-ts-v0.2.15   # TypeScript CLI (triggers: cli-ts-v*)
git tag data-ts-v0.1.0   # TypeScript data server (triggers: data-ts-v*)
git tag agent-v0.1.0     # TypeScript agent (triggers: agent-v*)
git push origin <tag>

# Option 2: Manual via GitHub CLI
gh workflow run build-connector.yaml -f tag=0.4.4
```

### Available Workflows

| Workflow | Image | Tag Format | Dockerfile |
|----------|-------|------------|------------|
| `build-connector.yaml` | `ghcr.io/aaronwald/ssmd-connector` | `v*` | `ssmd-rust/Dockerfile` |
| `build-archiver.yaml` | `ghcr.io/aaronwald/ssmd-archiver` | `v*` | `ssmd-rust/crates/ssmd-archiver/Dockerfile` |
| `build-operator.yaml` | `ghcr.io/aaronwald/ssmd-operator` | `operator-v*` | `ssmd-operators/Dockerfile` |
| `build-signal-runner.yaml` | `ghcr.io/aaronwald/ssmd-signal-runner` | `signal-runner-v*` | `ssmd-agent/Dockerfile.signal` |
| `build-cli-ts.yaml` | `ghcr.io/aaronwald/ssmd-cli-ts` | `cli-ts-v*` | `ssmd-agent/Dockerfile.cli` |
| `build-data-ts.yaml` | `ghcr.io/aaronwald/ssmd-data-ts` | `data-ts-v*` | `ssmd-agent/Dockerfile.data` |
| `build-agent.yaml` | `ghcr.io/aaronwald/ssmd-agent` | `agent-v*` | `ssmd-agent/Dockerfile` |

## Kubernetes Operator

The ssmd-operators project manages Kubernetes CRDs for market data components:

### CRDs

| CRD | Purpose |
|-----|---------|
| `connectors.ssmd.ssmd.io` | Manages Kalshi WebSocket connectors |
| `archivers.ssmd.ssmd.io` | Manages NATS → JSONL.gz archivers |
| `signals.ssmd.ssmd.io` | Manages signal evaluation pods |

### Operator Commands

```bash
# Generate CRDs after modifying *_types.go
cd ssmd-operators && make manifests

# Build locally
make build

# Tag and push to trigger image build
git tag operator-v0.4.1 && git push origin operator-v0.4.1
```

After updating the operator, copy CRDs to varlab:
`varlab/clusters/homelab/apps/ssmd/operator/crds.yaml`


## Instructions

1. All code must go through pr code review.
2. Follow the build instructions in CLAUDE.md. Limit freelancing unless we are in a brainstorming session.
