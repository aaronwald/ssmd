# ssmd: Kalshi Design - Roadmap

## Implementation Phases

### Phase 1: CLI & GitOps Metadata (COMPLETED)

GitOps-based metadata management - the system must know what it's managing before managing it.

**Completed:**
- [x] Go CLI setup with Cobra
- [x] `ssmd init` - creates directory structure
- [x] `ssmd feed create/list/show/add-version` - feed registry management
- [x] `ssmd schema register/list/show/set-status/hash` - schema registry
- [x] `ssmd env create/list/show/update` - environment management
- [x] `ssmd validate` - cross-file referential integrity
- [x] `ssmd diff/commit` - git workflow integration

**Bootstrap data:**
- [x] Register Kalshi feed in `exchanges/feeds/kalshi.yaml`
- [x] Register trade schema in `exchanges/schemas/trade.yaml`
- [x] Create dev environment in `exchanges/environments/kalshi-dev.yaml`

### Phase 2: Connector + Streaming

**Rust connector:**
- [ ] Rust project setup (cargo workspace)
- [ ] Cap'n Proto schema definition (.capnp files)
- [ ] Kalshi WebSocket client (tokio + tungstenite)
- [ ] Connector reads feed config from YAML files
- [ ] Basic NATS publisher (Cap'n Proto)

**Gateway:**
- [ ] Gateway subscribes to NATS
- [ ] Gateway serves WebSocket (JSON translation)
- [ ] REST API endpoints for market data

**Deliverable:** Live trades visible via WebSocket.

### Phase 3: Persistence + Inventory

**Archival:**
- [ ] Raw archiver (JSONL to S3)
- [ ] Normalized archiver (Cap'n Proto to S3)
- [ ] Archiver writes manifest.json on completion
- [ ] Gap detection: archiver records disconnections

**Security master sync:**
- [ ] Temporal workflow: sync markets from Kalshi API
- [ ] Store in Redis cache
- [ ] Publish change events to NATS journal

**Data inventory CLI:**
- [ ] `ssmd data inventory --feed kalshi` - show what data exists (reads manifests)
- [ ] `ssmd data gaps --feed kalshi --date DATE` - show gaps
- [ ] `ssmd data quality --feed kalshi --date DATE` - quality report

**Deliverable:** Data persists. Can query `ssmd data inventory` to see coverage.

### Phase 4: Operations + Scheduling

**Temporal workflows:**
- [ ] Daily startup workflow (sync secmaster → start connector → start archiver → start gateway)
- [ ] Daily teardown workflow (drain → flush → stop → verify)
- [ ] Workflow publishes events to journal

**Secrets + deployment:**
- [ ] Sealed Secrets integration
- [ ] ArgoCD manifests
- [ ] Key management CLI (`ssmd key set/list/verify`)

**Observability:**
- [ ] Prometheus metrics
- [ ] Alert rules

**CLI completion:**
- [ ] `ssmd day start/end/roll/status/history`
- [ ] `ssmd data replay --date DATE`

**Deliverable:** Production-ready. Daily cycle automated. Full audit trail.

## Open Questions

1. **Kalshi rate limits** - Need to verify API limits for market sync
2. **Orderbook depth** - Full book or top N levels?
3. **Historical backfill** - Does Kalshi provide historical data API?
4. **Client auth** - API keys sufficient or need more?

## Future Work (Post-Milestone)

- Polymarket connector
- Kraken connector (libechidna/C++ integration)
- TUI admin interface
- Agent feedback API for data quality issues
- Lua transforms for custom client formats
- Multi-tenant support
- MCP server for Claude integration

## Design Decisions Made

### GitOps over Database

Metadata (feeds, schemas, environments) is stored as YAML files in git, not in a database:
- Simpler operations - no schema migrations, backups, replicas
- Version control - full history with git blame, bisect, revert
- Code review - all changes go through PR review
- Reproducibility - clone repo = full metadata state

### Runtime State in Cache + Journal

Dynamic state (secmaster, trading day state, agent feedback) uses:
- Redis for fast lookups and current state
- NATS journal for audit trail and event streaming
- No database required

### Middleware Abstractions

All infrastructure dependencies (transport, storage, cache, journal) are behind traits:
- Swap implementations via config, not code
- In-memory implementations for testing
- Future-proof for new backends

---

*Design created: 2025-12-14*
*Last updated: 2025-12-22*
