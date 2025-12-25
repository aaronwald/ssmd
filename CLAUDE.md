# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ssmd - Simple/Streaming Market Data system

## Build Commands

```bash
# Build everything (Go + Rust)
make all-build

# Go only
make build

# Rust only
make rust-build
```

## Test Commands

```bash
# Test everything (Go + Rust)
make all-test

# Go only
make test

# Rust only
make rust-test
```

## Lint Commands

```bash
# Lint everything (Go + Rust)
make all-lint

# Go only
make lint

# Rust only
make rust-clippy
```

## Full Validation

```bash
# Run lint + test + build for both Go and Rust
make all
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

## Instructions

1. All code must go through pr code review.
1. Use idiomatic go. See .github/instructions/go.instructions.md
1. Follow the build instructions in CLAUDE. Limit freelancing unless we are in a brainstorming session.
