# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ssmd - Simple/Streaming Market Data system

## Build Commands

All builds are done via Makefile:

```bash
# Full validation (lint + security + test + build)
make all

# Build everything (Go CLI + Rust + ssmd-data)
make all-build

# Test everything (Go + Rust)
make all-test

# Lint everything (Go + Rust)
make all-lint
```

### Go Targets

```bash
make build          # Build ssmd CLI
make test           # Run Go tests
make lint           # Run vet + staticcheck
make security       # Run govulncheck
```

### Rust Targets

```bash
make rust-build     # Build all Rust crates
make rust-test      # Run Rust tests
make rust-clippy    # Run Clippy linter
make rust-clean     # Clean Rust build artifacts
```

### ssmd-data API

```bash
make data-build     # Build ssmd-data binary to bin/
make data-test      # Run API handler tests
make data-run       # Run with test config (SSMD_DATA_PATH=./testdata)
```

### ssmd-agent (Deno)

```bash
make agent-check    # Deno type check
make agent-test     # Run agent tests
make agent-run      # Start agent REPL (requires ANTHROPIC_API_KEY)
```

## Prerequisites

```bash
# Install Cap'n Proto compiler (required for ssmd-schema crate)
sudo apt-get install -y capnproto

# Install Rust (if not present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
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

Images build automatically on git tag push (e.g., `v0.4.4`), or via manual workflow dispatch.

### Triggering a Build

```bash
# Option 1: Tag and push (triggers build-cli.yaml, build-connector.yaml, etc.)
git tag v0.4.4
git push origin v0.4.4

# Option 2: Manual via GitHub CLI
gh workflow run build-cli.yaml -f tag=0.4.4
```

### Available Workflows

| Workflow | Image | Dockerfile |
|----------|-------|------------|
| `build-cli.yaml` | `ghcr.io/aaronwald/ssmd` | `cmd/ssmd/Dockerfile` |
| `build-connector.yaml` | `ghcr.io/aaronwald/ssmd-connector` | `ssmd-rust/Dockerfile` |
| `build-archiver.yaml` | `ghcr.io/aaronwald/ssmd-archiver` | `ssmd-rust/crates/ssmd-archiver/Dockerfile` |
| `build-data.yaml` | `ghcr.io/aaronwald/ssmd-data` | `cmd/ssmd-data/Dockerfile` |
| `build-agent.yaml` | `ghcr.io/aaronwald/ssmd-agent` | `ssmd-agent/Dockerfile` |

### Updating ssmd-worker (Temporal)

The ssmd-worker in varlab bundles the ssmd CLI. After pushing a new ssmd tag:

1. Update `varlab/workers/kalshi-temporal/Dockerfile` to reference new ssmd version
2. Build and push new ssmd-worker image
3. Update `varlab/clusters/homelab/apps/ssmd/worker/deployment.yaml`

## Instructions

1. All code must go through pr code review.
1. Use idiomatic go. See .github/instructions/go.instructions.md
1. Follow the build instructions in CLAUDE. Limit freelancing unless we are in a brainstorming session.
