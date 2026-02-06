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

| Task Domain | ssmd Experts | + General Experts |
|-------------|--------------|-------------------|
| Market metadata work | Secmaster, CLI | Database, Senior Dev |
| Adding new exchange | Data Feed | DevOps, Security |
| Trading API integration | Access Feed, Secmaster | Security, API Designer |
| Pipeline deployment | Data Feed, CLI | DevOps, Platform/Infra |
| Signal development | Data Feed, Secmaster, CLI | Performance, QA |
| CDC/cache work | Secmaster | Database, Performance |
| Orderbook integration | Access Feed, Data Feed | Performance, Security |
| CLI operations | CLI | DevOps |
| Data quality checks | Data Quality, CLI | QA |

## Instructions

### Step 1: Understand the Task

Same as waldstorm - gather task description, constraints, context.

### Step 2: Select Experts

**Always include at least one ssmd expert.** Use the selection guide above.

For ssmd-specific tasks, read the relevant expert files from `./experts/`:
- `secmaster.md` - Market metadata, sync, CDC
- `data-feed.md` - Connectors, NATS, archiving
- `access-feed.md` - Trading APIs, orderbook, fills
- `kalshi.md` - Kalshi exchange domain knowledge, API, fees
- `data-quality.md` - NATS vs API reconciliation, trade verification
- `cli.md` - ssmd CLI commands (Deno ops + Go GitOps), env management

Combine with waldstorm's general experts (Security, DevOps, etc.) as needed.

### Step 3-7: Follow waldstorm

Use team primitives (TeamCreate, TaskCreate, Task tool for teammates, SendMessage) to run expert analysis, then synthesize, plan, and execute per waldstorm workflow. Clean up with shutdown_request + TeamDelete.

## Key ssmd Context

**Architecture:**
```
Kalshi WS -> Connector -> NATS JetStream -> Archiver/Signal/Notifier
                              |
PostgreSQL <- secmaster sync <- CDC -> dynamic subscriptions
```

**Current state (Feb 2026):**
- Subscribed channels: `ticker`, `trade`, `market_lifecycle_v2`
- Pending: `orderbook_delta`, `fill`, `market_positions`
- Environments: prod (homelab k3s), dev (GKE)

**Key paths:**
- ssmd code: project root
- K8s manifests: `varlab/clusters/homelab/apps/ssmd/` (in 899bushwick/varlab)
- Runbooks: `varlab/docs/runbooks/apps/ssmd*.md` (in 899bushwick/varlab)
- CLI reference: `docs/reference/cli-reference.md` (in 899bushwick)

## Superpowers Used

- `superpowers:writing-plans`
- `superpowers:executing-plans`
