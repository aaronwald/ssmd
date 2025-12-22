# Phase 1: GitOps Metadata Foundation

> Revised design replacing database-first approach with git-native configuration management.

## Overview

Phase 1 creates a git-native CLI for managing ssmd configuration. All metadata lives as files in the repository. No database or runtime infrastructure required for development.

**Core principle:** Git is the source of truth. The CLI reads and writes files, then commits changes.

## Goals

- Define file structure for feeds, schemas, environments
- Build `ssmd` CLI that manages these files
- Validate referential integrity across configuration
- Git commit workflow for all changes
- Zero infrastructure dependencies for development

## Non-Goals (Deferred)

- Runtime infrastructure (etcd, databases)
- Operational data (gaps, inventory, trading day status)
- Actual connectors and data collection
- Key/secret storage (only references defined)

## Architecture

```
Developer                    Git Repository
    │                              │
    │  ssmd feed create kalshi     │
    │─────────────────────────────>│
    │                              │  writes feeds/kalshi.yaml
    │                              │
    │  ssmd schema register trade  │
    │─────────────────────────────>│
    │                              │  writes schemas/trade.yaml
    │                              │         schemas/trade.capnp
    │  ssmd validate               │
    │─────────────────────────────>│
    │                              │  checks referential integrity
    │  ssmd commit -m "..."        │
    │─────────────────────────────>│
    │                              │  git add + git commit
```

**Development:** Pure file operations. Clone repo, run CLI, commit changes.

**Deployment (future):** Config loaded from git into etcd at deploy time for runtime access.

## File Structure

```
ssmd/
├── feeds/
│   ├── kalshi.yaml
│   └── polymarket.yaml
├── schemas/
│   ├── trade.capnp
│   ├── trade.yaml
│   ├── orderbook.capnp
│   └── orderbook.yaml
├── environments/
│   ├── kalshi-dev.yaml
│   └── kalshi-prod.yaml
└── .ssmd/
    └── config.yaml      # local CLI config (gitignored)
```

| Directory | Purpose |
|-----------|---------|
| `feeds/` | One YAML file per data source. Connection details, capabilities, version history. |
| `schemas/` | Pairs of `.capnp` (definition) and `.yaml` (metadata) files. |
| `environments/` | Self-contained deployment configs. Everything needed to run. |
| `.ssmd/` | Local CLI state (gitignored). User preferences, cached validations. |

## Validation

`ssmd validate` checks referential integrity without reaching external systems:

- Feed: required fields, valid versions, non-overlapping dates
- Schema: file exists, hash matches, valid status
- Environment: referenced feed exists, referenced schema is active, key fields defined

## Git Workflow

Changes are batched until explicitly committed:

```bash
ssmd feed create kalshi --type websocket ...
ssmd schema register trade --file schemas/trade.capnp
ssmd env create kalshi-dev --feed kalshi
ssmd diff                    # review pending changes
ssmd validate                # check integrity
ssmd commit -m "Add Kalshi"  # stage and commit
```

`ssmd commit` runs validation, stages ssmd files, and commits. Does not push.

## Key Management

Environments reference keys by name and source:

| Source | Example | Use Case |
|--------|---------|----------|
| `env` | `SSMD_KALSHI_API_KEY` | Local development |
| `sealed-secret/name` | `sealed-secret/kalshi-creds` | Kubernetes production |
| `vault/path` | `vault/ssmd/kalshi` | HashiCorp Vault |

Actual secrets never stored in git. Only references.

## Tech Stack

- **Language:** Go
- **CLI framework:** Cobra
- **File format:** YAML (config), Cap'n Proto (schemas)
- **Validation:** Internal (no external dependencies)

## Related Documents

- [File Format Reference](./2025-12-16-phase1-file-formats.md) — Detailed YAML specifications
- [CLI Reference](./2025-12-16-phase1-cli-reference.md) — Commands and flags

## What Comes Next

**Phase 2:** Runtime layer
- etcd for intraday configuration
- Operational state storage
- Trading day lifecycle management

**Phase 3:** Data collection
- Connectors per feed
- Normalization pipeline
- Storage and archival
