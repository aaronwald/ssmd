# ssmd: Kalshi Design - Overview

## Overview

ssmd is a homelab-friendly market data system. It captures live market data, streams it for real-time consumption, and archives it for backtesting. Simple enough to admin via TUI, simple enough for AI agents to query.

**First milestone:** End-to-end Kalshi streaming.

## Goals

- Capture live Kalshi data and stream to clients (AI agent, trading bot, TUI)
- Archive raw and normalized data to S3-compatible storage
- Daily teardown/startup cycle - no long-running state
- Learn Rust and Cap'n Proto on a real project

## Non-Goals

- Ultra-low-latency (Kalshi is WebSocket, not binary multicast)
- Custom tickerplant (stateful order book routing) - defer state to edge, use transport layer for routing

## First Milestone Scope

Kalshi only. Polymarket and Kraken follow after the foundation is proven.

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Kalshi    │────▶│  Connector  │────▶│    NATS     │
│  WebSocket  │     │   (Rust)    │     │  JetStream  │
└─────────────┘     └──────┬──────┘     └──────┬──────┘
                           │                   │
                           ▼                   ├────────────────┐
                    ┌─────────────┐            │                │
                    │ Raw Archive │            ▼                ▼
                    │   (JSONL)   │     ┌─────────────┐  ┌─────────────┐
                    └──────┬──────┘     │  Archiver   │  │   Gateway   │
                           │            │   (Rust)    │  │   (Rust)    │
                           ▼            └──────┬──────┘  └──────┬──────┘
                    ┌─────────────┐            │                │
                    │     S3      │◀───────────┘                │
                    │ (raw/norm)  │                             ▼
                    └─────────────┘                      ┌─────────────┐
                                                         │   Clients   │
                                                         │ (WS + JSON) │
                                                         └─────────────┘
```

## Components

| Component | Language | Purpose |
|-----------|----------|---------|
| ssmd-connector | Rust | Kalshi WebSocket → NATS (Cap'n Proto) |
| ssmd-archiver | Rust | NATS → S3 normalized storage |
| ssmd-gateway | Rust | NATS → WebSocket (JSON for clients) |
| ssmd-cli | Go | Environment management, operations |
| ssmd-worker | Go | Temporal workflows for scheduling |

### Why Rust

- Learning goal: build production Rust skills on a real project
- Good fit for streaming data with async/await (tokio)
- Cap'n Proto has solid Rust support
- Sets foundation for higher-performance markets (Kraken) later

### Why Go for Tooling

- Faster iteration for CLI (cobra ecosystem)
- Temporal SDK is mature in Go
- Already specified in design brief

### Future: Zig

Zig is noted as a future option in the design brief. Potential use cases:

- **Low-latency components** - Zero-overhead interop with C (libechidna)
- **WASM builds** - Browser-based replay/visualization tools
- **Embedded systems** - Resource-constrained edge deployments

Not used in Phase 1, but the middleware abstractions keep this door open. The trait-based design means a Zig component could implement the same interfaces.
