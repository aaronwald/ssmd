# ssmd - Stupid Simple Market Data

A homelab-friendly market data system. Capture, archive, and analyze market data with GitOps configuration and AI-powered signal development.

## Vision

- **Simple enough for homelab** - No enterprise infrastructure required
- **Simple enough for a TUI** - Minimal operational complexity
- **Simple enough for Claude** - Easy to define new skills and integrations
- **Cloud-first** - Kubernetes-native, GitOps-driven
- **Quality data platform** - Provenance tracking, versioned schemas, gap detection

## Architecture

```
Exchanges          Homelab Infrastructure                    Developer Laptop
─────────          ──────────────────────                    ────────────────
                   ┌─────────────────────────────────────┐
 Kalshi ──────────▶│  Connector ──▶ NATS ──▶ Archiver   │
 Polymarket ──────▶│    (Rust)    JetStream    (Rust)   │
 Kraken ──────────▶│                  │          │       │
                   │                  │          ▼       │
                   │                  │    JSONL.gz      │
                   │                  │    + Manifest    │
                   │                  │          │       │
                   │                  │          ▼       │
                   │                  │    ssmd-data ◀───│───── ssmd-agent
                   │                  │     (Go API)     │      (Deno REPL)
                   │                  │                  │
                   │  ssmd CLI (Go) - Metadata mgmt     │
                   └─────────────────────────────────────┘
```

## Components

| Component | Language | Purpose |
|-----------|----------|---------|
| **ssmd** | Go | CLI for GitOps metadata management |
| **ssmd-connector** | Rust | WebSocket → NATS publisher |
| **ssmd-archiver** | Rust | NATS → JSONL.gz files |
| **ssmd-data** | Go | HTTP API for archived data |
| **ssmd-agent** | Deno | LangGraph REPL for signal development |

## Quick Start

```bash
# Prerequisites
sudo apt-get install -y capnproto
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Build and test everything
make all
```

## Documentation

| Document | Purpose |
|----------|---------|
| [CLAUDE.md](CLAUDE.md) | Build commands and development guide |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Kubernetes deployment |
| [AGENT.md](AGENT.md) | Signal development with ssmd-agent |
| [TODO.md](TODO.md) | Task tracking and roadmap |
| [docs/designs/](docs/designs/) | Architecture and design documents |
| [docs/plans/](docs/plans/) | Implementation plans |
| [docs/reference/](docs/reference/) | CLI reference, file formats |

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Go for CLI | `ssmd` | Fast builds, single binary |
| Rust for hot path | Connector, archiver | Zero-cost abstractions |
| NATS JetStream | Transport | Persistence, replay, simple ops |
| GitOps | Config | Versioned, reviewable, auditable |
| JSONL.gz | Archive | Grep-friendly, compressed |
| Deno + LangGraph | Agent | Native TS, stateful tools |

## Current Status

See [TODO.md](TODO.md) for detailed status. Summary:

- **Phase 1** (GitOps Metadata): Complete
- **Phase 2** (NATS Streaming): Complete
- **Phase 3** (Agent Pipeline): Complete
- **Phase 4+** (Gateway, Security Master, Trading Day): Planned

## License

MIT
