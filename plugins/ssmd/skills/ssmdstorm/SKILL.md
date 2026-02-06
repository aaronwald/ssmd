---
name: ssmdstorm
description: Use when working on ssmd market data system tasks - extends waldstorm with domain-specific experts for secmaster, data feeds, trading APIs, and data quality
---

# ssmdstorm

Multi-agent orchestration for ssmd market data system work. Extends waldstorm with domain-specific experts.

## Overview

ssmdstorm adds 6 ssmd-specific experts to waldstorm's general panel:

| Expert | Focus Area |
|--------|------------|
| Secmaster | Market metadata, sync, lifecycle, CDC, Redis cache |
| Data Feed | Connectors, WebSocket, NATS streams, sharding |
| Access Feed | Trading APIs, orderbook, fills, positions |
| Kalshi | Kalshi exchange domain: API, fees, market mechanics |
| Data Quality | NATS vs API reconciliation, trade verification |
| CLI | ssmd CLI commands (Deno ops + Go GitOps), env management |

## Expert Selection Guide

| Task Domain | ssmd Experts | + General Experts | + varlab-ops |
|-------------|--------------|-------------------|--------------|
| Market metadata work | Secmaster, CLI | Database, Senior Dev | - |
| Adding new exchange | Data Feed | DevOps, Security | Operations |
| Trading API integration | Access Feed, Secmaster | Security, API Designer | - |
| Pipeline deployment | Data Feed, CLI | DevOps, Platform/Infra | Operations |
| Signal development | Data Feed, Secmaster, CLI | Performance, QA | - |
| CDC/cache work | Secmaster | Database, Performance | - |
| Orderbook integration | Access Feed, Data Feed | Performance, Security | - |
| CLI operations | CLI | DevOps | - |
| Data quality checks | Data Quality, CLI | QA | - |
| End-to-end deploy (code + K8s) | Data Feed, CLI | Security, Performance | Operations |

### Cross-Skill Pairing

Tasks that span code AND infrastructure need experts from **both** ssmdstorm and varlab-ops. The selection guide above includes the varlab-ops column for these cases. Common cross-skill patterns:

| Pattern | Why Both Skills |
|---------|----------------|
| New exchange end-to-end | ssmd: connector code, NATS subjects, writer. varlab-ops: K8s deployment, NATS stream, archiver CR, network policy |
| Image build + deploy | ssmd: Rust/Deno changes, tag format. varlab-ops: deployment YAML, Flux reconcile, image pull |
| Scale operations | ssmd: CLI commands. varlab-ops: Flux suspend/resume, kubectl context |

### Trigger Keywords

When analyzing a task description, match these keywords to experts:

| Keywords | Expert |
|----------|--------|
| websocket, connector, exchange, feed, subscribe, channel | Data Feed |
| secmaster, market metadata, sync, CDC, Redis, lifecycle | Secmaster |
| orderbook, fill, position, trading, order, balance | Access Feed |
| kalshi, prediction market, contract, series, category | Kalshi |
| nats count, match rate, reconciliation, missing trades | Data Quality |
| cli, scale, deploy, env, schedule, deno task | CLI |
| deployment, kustomization, flux, network policy, PVC | Operations (varlab-ops) |
| securityContext, input validation, sanitization, auth | Security (waldstorm) |
| max_message_size, buffer, latency, throughput, memory | Performance (waldstorm) |

## Instructions

### Step 1: Understand the Task

Same as waldstorm - gather task description, constraints, context.

### Step 2: Select Experts

**Always include at least one ssmd expert.** Use the selection guide above.

**Agent definitions** in `agents/` provide each ssmd expert as a spawnable agent with `memory: local` for persistent learnings across sessions.

Available ssmd agents (in `./agents/`):
- `ssmd-secmaster` - Market metadata, sync, CDC
- `ssmd-data-feed` - Connectors, NATS, archiving
- `ssmd-access-feed` - Trading APIs, orderbook, fills
- `ssmd-kalshi` - Kalshi exchange domain knowledge, API, fees
- `ssmd-data-quality` - NATS vs API reconciliation, trade verification
- `ssmd-cli` - ssmd CLI commands (Deno ops + Go GitOps), env management

Combine with waldstorm's general agents (Security, DevOps, etc.) as needed.

### Step 3-7: Follow waldstorm

Use team primitives (TeamCreate, TaskCreate, Task tool for teammates, SendMessage) to run expert analysis, then synthesize, plan, and execute per waldstorm workflow. Clean up with shutdown_request + TeamDelete.

### Step 8: Track Records (automated via agent memory)

Agents with `memory: local` automatically read/write their own MEMORY.md in `.claude/agent-memory-local/<name>/MEMORY.md`. Manual track record updates are no longer needed when using agents.

The **Expert Track Record** table below serves as historical reference. Agents now self-update their learnings via `memory: local`.

## Expert Track Record

Track which experts produced actionable findings per task type. Use this to inform future selection.

| Session | Task | Experts Used | Key Findings |
|---------|------|-------------|--------------|
| 2026-02-06 | Kraken exchange (end-to-end) | Data Feed, Operations, Security, Performance | Security: missing securityContext, NATS subject injection via unsanitized symbols. Performance: WebSocket max_message_size default too large. Data Feed: archiver-per-exchange, serde untagged pitfalls. Operations: static Deployment vs CR, Flux suspend lifecycle. All 4 experts produced HIGH-priority findings. |

**Observations:**
- End-to-end exchange tasks need 4+ experts across skills (cross-skill pairing essential)
- Security and Performance generals caught infra/config issues the domain experts missed
- Data Feed expert most valuable for exchange-specific protocol details
- Operations expert essential whenever K8s manifests are involved

## Key ssmd Context

**Architecture:**
```
Kalshi WS  -> Connector -> NATS JetStream -> Archiver/Signal/Notifier
Kraken WS  -> Connector ---^
                              |
PostgreSQL <- secmaster sync <- CDC -> dynamic subscriptions
```

**Current state (Feb 2026):**
- Exchanges: Kalshi (prediction markets), Kraken (crypto spot)
- Kalshi channels: `ticker`, `trade`, `market_lifecycle_v2`
- Kraken channels: `ticker`, `trade`
- Pending: `orderbook_delta`, `fill`, `market_positions` (Kalshi)
- Environments: prod (homelab k3s), dev (GKE)

**Key paths:**
- ssmd code: project root
- K8s manifests: `varlab/clusters/homelab/apps/ssmd/` (in 899bushwick/varlab)
- Runbooks: `varlab/docs/runbooks/apps/ssmd*.md` (in 899bushwick/varlab)
- CLI reference: `docs/reference/cli-reference.md` (in 899bushwick)

## Superpowers Used

- `superpowers:writing-plans`
- `superpowers:executing-plans`
