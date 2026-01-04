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

## Prerequisites

```bash
# Install Cap'n Proto compiler (required for ssmd-schema crate)
sudo apt-get install -y capnproto

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
| `build-cli-ts.yaml` | `ghcr.io/aaronwald/ssmd-cli-ts` | `cli-ts-v*` | `ssmd-agent/Dockerfile.cli` |
| `build-data-ts.yaml` | `ghcr.io/aaronwald/ssmd-data-ts` | `data-ts-v*` | `ssmd-agent/Dockerfile.data` |
| `build-agent.yaml` | `ghcr.io/aaronwald/ssmd-agent` | `agent-v*` | `ssmd-agent/Dockerfile` |


## Instructions

1. All code must go through pr code review.
2. Follow the build instructions in CLAUDE.md. Limit freelancing unless we are in a brainstorming session.
