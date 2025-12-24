# SSMD TODO

## Completed

### Phase 1: GitOps Metadata Foundation (2025-12-19)
- [x] Task 1: Project Setup - Go module, Cobra CLI, main.go
- [x] Task 2: Init Command - `ssmd init` creates directory structure
- [x] Task 3: Feed Types - Feed structs, YAML parsing, validation
- [x] Task 4: Feed Commands - list, show, create, update, add-version
- [x] Task 5: Schema Types - Schema structs, SHA256 hash computation
- [x] Task 6: Schema Commands - list, show, register, hash, set-status, add-version
- [x] Task 7: Environment Types - Environment, transport, storage, key configs
- [x] Task 8: Environment Commands - list, show, create, update, add-key
- [x] Task 9: Validation Command - Cross-file referential integrity
- [x] Task 10: Git Commands - diff and commit workflow

### Code Review & Enhancements (2025-12-20)
- [x] Fix staticcheck S1002 issue in environment.go
- [x] Fix nil pointer risk in env list command
- [x] Remove unused --quiet/--verbose flags
- [x] Add proper error handling to directory functions
- [x] Extract duplicate sorting logic (SortVersionsDesc)
- [x] Fix schema version file tracking in add-version
- [x] Add capture_locations to feeds for provenance
- [x] Add `ssmd feed add-location` command
- [x] Add effective_to dates for version date ranges
- [x] Create README with Kalshi feed example
- [x] Create PR #1 for provenance features (MERGED)

### Cleanup & Bootstrap (2025-12-22)
- [x] Add govulncheck to Makefile
- [x] Bootstrap Kalshi configuration (exchanges/feeds/, exchanges/schemas/, exchanges/environments/)
- [x] Add end-to-end CLI workflow tests
- [x] Reorganize docs: reference docs moved to docs/reference/
- [x] Archive completed implementation plans
- [x] Restructure directories: move configs under exchanges/
- [x] Create Claude skill for ssmd CLI documentation
- [x] Create PR #4 for exchanges restructure

### Key Management (2025-12-22)
- [x] Add key types (KeyStatus, KeyType) to internal/types
- [x] Add tls/webhook key types, description field to KeySpec
- [x] Implement `ssmd key list <env>` - list keys with sources
- [x] Implement `ssmd key show <env> <key>` - show key details, verify env vars
- [x] Implement `ssmd key verify <env>` - verify all keys in environment
- [x] Implement `ssmd key check <env> <key>` - check single key
- [x] Add `--check-keys` flag to `ssmd validate`
- [x] Security: ssmd never stores secrets, only validates external sources
- [x] Create PR #6 for key management (MERGED)

### Rust Runtime Framework (2025-12-22)
- [x] Design: `docs/plans/2025-12-22-runtime-framework-design.md`
- [x] ssmd-rust Cargo workspace structure
- [x] ssmd-metadata crate (Feed, Schema, Environment types)
- [x] ssmd-connector crate (Connector trait, WebSocket implementation)
- [x] ssmd-connector binary entry point
- [x] Makefile Rust targets (rust-build, rust-test, rust-clippy, all-*)
- [x] Create PR #8 for Rust runtime (MERGED)

### Schema Normalization (2025-12-22)
- [x] Design: `docs/plans/2025-12-22-schema-normalization-design.md`
- [x] Protocol normalization (TransportProtocol + MessageProtocol)
- [x] CaptureLocation generalization (site + SiteType)
- [x] Go types + validation + tests
- [x] Rust types + tests
- [x] CLI command updates
- [x] Included in PR #8 (MERGED)

### Middleware & Cap'n Proto (2025-12-23)
- [x] Design: `docs/plans/2025-12-23-middleware-capnproto.md`
- [x] ssmd-middleware crate (Transport, Storage, Cache, Journal traits)
- [x] In-memory implementations for all middleware traits
- [x] MiddlewareFactory for runtime selection based on Environment config
- [x] ssmd-schema crate with Cap'n Proto trade types
- [x] Publisher integration in ssmd-connector
- [x] Updated CLAUDE.md with build/test/lint commands
- [x] 37 Rust tests passing

### Latency Optimizations (2025-12-23)
- [x] Design: `docs/plans/2025-12-23-latency-optimizations-design.md`
- [x] TSC timestamps via quanta (~10ns vs ~50ns syscall)
- [x] Lock-free channels with DashMap + AtomicU64
- [x] String interning via lasso ThreadedRodeo
- [x] SPSC mmap ring buffer (4MB, 1024 slots)
- [x] Disk flusher with batching on dedicated thread
- [x] 66 Rust tests passing

### Kalshi Connector (2025-12-23)
- [x] Design: `docs/plans/2025-12-23-kalshi-port-impl.md`
- [x] Kalshi auth module (RSA-PSS signing)
- [x] Kalshi message types (WsMessage, TradeData, TickerData, OrderbookData)
- [x] Kalshi WebSocket client (connect, subscribe, recv)
- [x] KalshiConnector implementing Connector trait
- [x] KalshiConfig from environment variables
- [x] Binary entry point updated for Kalshi
- [x] Environment config updated (kalshi-dev.yaml)
- [x] 58 Rust tests passing

## In Progress

_None_

## Pending

### Phase 2: Streaming & Gateway
Ref: `docs/plans/designs/kalshi/13-roadmap.md`, `01-overview.md`, `05-data-flow.md`

**Connector (partially complete):**
- [x] Rust project setup (cargo workspace)
- [x] Cap'n Proto schema definition (.capnp files)
- [x] Kalshi WebSocket client (tokio + tungstenite)
- [x] Connector reads feed config from YAML files
- [ ] NATS publisher (Cap'n Proto) - currently file output only
- [ ] Environment prefix keying for NATS subjects (`{env}.{feed}.{type}.{symbol}`)

**ssmd-gateway (Rust):**
Ref: `docs/plans/designs/kalshi/05-data-flow.md`, `09-error-handling.md`
- [ ] Gateway crate setup
- [ ] NATS subscription (Cap'n Proto)
- [ ] WebSocket server (JSON translation)
- [ ] REST API endpoints (`/v1/markets`, `/v1/markets/{ticker}/trades`, `/v1/health`)
- [ ] Client connection management with bounded buffers
- [ ] Backpressure handling (drop policy, conflation)
- [ ] Subscription modes (realtime, conflated, latest)

**ssmd-archiver (Rust):**
Ref: `docs/plans/designs/kalshi/01-overview.md`, `05-data-flow.md`
- [ ] Archiver crate setup
- [ ] NATS subscription for raw/normalized data
- [ ] Raw archiver (JSONL to S3, compressed)
- [ ] Normalized archiver (Cap'n Proto to S3)
- [ ] Manifest file writing on completion
- [ ] Gap detection and recording in manifest

**Sequenced Stream Handling:**
Ref: `docs/plans/completed/2025-12-22-schema-normalization.md` TODO section.
- [ ] Add `sequenced: bool` to Protocol struct
- [ ] Add `sequence_field: string` to Protocol struct
- [ ] Sequence number tracking in Rust connector
- [ ] Gap detection and alerting
- [ ] Recovery mechanisms (where protocol supports)

### Phase 3: Persistence & Inventory
Ref: `docs/plans/designs/kalshi/13-roadmap.md`, `06-security-master.md`, `02-key-management.md`

**Security Master Sync:**
Ref: `docs/plans/designs/kalshi/06-security-master.md`
- [ ] Market data model (Market struct with status, timing, settlement)
- [ ] Redis cache layout (`{env}:secmaster:markets`, by_category, expiring)
- [ ] Sync job: fetch from Kalshi API, compute changes, update cache
- [ ] Change journal: publish to `{env}.secmaster.changes`
- [ ] Connector integration: validate symbols against secmaster
- [ ] Expiration handling: unsubscribe from settled markets
- [ ] Cache warming on startup
- [ ] CLI: `ssmd secmaster sync <env>` - trigger manual sync
- [ ] CLI: `ssmd secmaster list <env>` - list markets with filters
- [ ] CLI: `ssmd secmaster show <env> <ticker>` - market details
- [ ] CLI: `ssmd secmaster search <env> <query>` - search markets
- [ ] CLI: `ssmd secmaster export <env>` - export for backup

**Key Management Enhancements:**
Ref: `docs/plans/designs/kalshi/02-key-management.md`
- [x] Key types and validation (completed in Phase 1)
- [x] `ssmd key list/show/verify/check` (completed)
- [ ] `ssmd key set <env> <key>` - set key values (Sealed Secrets)
- [ ] `ssmd key init <env>` - interactive key setup
- [ ] `ssmd key rotate <env> <key>` - rotate key values
- [ ] `ssmd key delete <env> <key>` - delete a key
- [ ] `ssmd key export <env>` - export key references (no secrets)
- [ ] Runtime KeyResolver (Rust) for Sealed Secrets lookup
- [ ] Key expiration tracking and Prometheus alerts

**Data Inventory CLI:**
Ref: `docs/plans/designs/kalshi/03-metadata-gitops.md`
- [ ] `ssmd data inventory --feed kalshi` - show what data exists (reads S3 manifests)
- [ ] `ssmd data gaps --feed kalshi --date DATE` - show gaps
- [ ] `ssmd data quality --feed kalshi --date DATE` - quality report
- [ ] `ssmd env teardown <env>` - delete all env-prefixed data (S3, NATS, Redis)

### Phase 4: Operations & Scheduling
Ref: `docs/plans/designs/kalshi/13-roadmap.md`, `07-trading-day.md`, `08-sharding.md`, `09-error-handling.md`

**Trading Day Management:**
Ref: `docs/plans/designs/kalshi/07-trading-day.md`
- [ ] Trading day state machine (PENDING → STARTING → ACTIVE → ENDING → COMPLETE)
- [ ] State storage in Redis (`{env}:day:current`, `{env}:day:{date}:state`)
- [ ] Day events journal (`{env}.day.events`)
- [ ] CLI: `ssmd day status` - current trading day status
- [ ] CLI: `ssmd day start <env>` - start trading day (triggers workflow)
- [ ] CLI: `ssmd day end <env>` - end trading day (triggers teardown)
- [ ] CLI: `ssmd day roll <env>` - end current + start next
- [ ] CLI: `ssmd day history <env>` - view day history from journal
- [ ] CLI: `ssmd day show <env> <date>` - specific day details
- [ ] CLI: `ssmd day recover <env>` - resume from last checkpoint
- [ ] Data partitioning by trading day

**Temporal Workflows (ssmd-worker Go):**
Ref: `docs/plans/designs/kalshi/01-overview.md`, `07-trading-day.md`
- [ ] ssmd-worker Go module setup
- [ ] StartTradingDay workflow (sync → connect → start archiver → start gateway → health check)
- [ ] EndTradingDay workflow (drain → flush → stop → verify → record)
- [ ] RollTradingDay workflow (end current + start next)
- [ ] Workflow publishes events to journal
- [ ] Scheduled operations via environment config

**Sharding & Scaling:**
Ref: `docs/plans/designs/kalshi/08-sharding.md`
- [ ] Symbol attributes in metadata (tier, category)
- [ ] Shard definitions in environment YAML (selectors, replicas, resources)
- [ ] Shard resolution from secmaster at startup
- [ ] NATS subject sharding (`internal.{shard}.{feed}.{type}.{symbol}`)
- [ ] NATS stream mirroring for client-facing subjects
- [ ] Auto-scaling configuration (Kubernetes HPA)
- [ ] Fixed memory profile components (bounded buffers, LRU caches)
- [ ] CLI: `ssmd shard list <env>` - list shards with metrics
- [ ] CLI: `ssmd shard show <env> <shard>` - shard details
- [ ] CLI: `ssmd shard symbols <env>` - symbol → shard mapping
- [ ] CLI: `ssmd shard move <env> <symbol>` - move symbol between shards
- [ ] CLI: `ssmd shard plan <env>` - preview resharding
- [ ] CLI: `ssmd shard apply <env>` - execute resharding plan

**Error Handling & Resilience:**
Ref: `docs/plans/designs/kalshi/09-error-handling.md`
- [ ] Retry policy with exponential backoff + jitter
- [ ] Dead letter queue (`{env}.dlq.{component}`)
- [ ] Circuit breaker for downstream calls
- [ ] Graceful degradation (cache bypass, archiver catch-up)
- [ ] CLI: `ssmd dlq list` - view dead letters
- [ ] CLI: `ssmd dlq replay <id>` - replay failed message
- [ ] CLI: `ssmd dlq purge` - purge old dead letters
- [ ] CLI: `ssmd client list` - view gateway clients
- [ ] CLI: `ssmd client disconnect <id>` - force disconnect
- [ ] CLI: `ssmd client set-mode <id>` - change subscription mode

**Secrets & Deployment:**
Ref: `docs/plans/designs/kalshi/12-deployment.md`
- [ ] Sealed Secrets integration
- [ ] ArgoCD manifests for ssmd
- [ ] Kubernetes namespace setup

**Observability:**
Ref: `docs/plans/designs/kalshi/12-deployment.md`
- [ ] Prometheus metrics: connector (messages, lag, errors)
- [ ] Prometheus metrics: gateway (clients, subscriptions, messages)
- [ ] Prometheus metrics: archiver (bytes, files written)
- [ ] Latency histograms (P50/P95/P99)
- [ ] Alert rules (no data, high lag, circuit breaker, DLQ accumulating)
- [ ] Structured JSON logging to stdout

**CLI Completion:**
- [ ] `ssmd data replay --date DATE --symbol SYMBOL`
- [ ] `ssmd data export --date DATE --format parquet`

### Agent Pipeline Implementation
Design: `docs/plans/designs/2025-12-23-agent-pipeline.md`

**Signal Runtime (Deno):**
- [ ] Deno project setup with LangGraph.js
- [ ] State Builders (orderbook, priceHistory, volumeProfile)
- [ ] Signal interface and evaluator
- [ ] NATS subscription for raw market data
- [ ] Signal event publishing to NATS

**Definition Agent:**
- [ ] LangGraph graph for signal creation
- [ ] Structured output + template for signal generation
- [ ] `create_signal` tool with schema validation
- [ ] Deno type-check validation
- [ ] Git commit workflow for signal deployment

**Action Agent:**
- [ ] LangGraph graph for signal response
- [ ] Interpret → Decide → Execute nodes
- [ ] Action types (alert, log, webhook, trade_signal)
- [ ] Action event publishing to NATS

**Agent Tools:**
- [ ] `replay_orderbook` - build state from archived deltas
- [ ] `list_state_builders` - show available builders
- [ ] `list_signals` - show existing signals
- [ ] `get_recent_trades` - query NATS history
- [ ] `get_signal_history` - query signal fire events

**Replay Mode:**
- [ ] NatsReplay for historical data testing
- [ ] S3 archive replay support
- [ ] Signal testing against replayed data

### MCP Server
Ref: `docs/plans/designs/kalshi/10-agent-integration.md`

- [ ] ssmd-mcp Go server implementing MCP protocol
- [ ] Tools: `ssmd_list_markets`, `ssmd_get_market`, `ssmd_get_trades`
- [ ] Tools: `ssmd_get_orderbook`, `ssmd_query_historical`
- [ ] Tools: `ssmd_report_issue`, `ssmd_system_status`, `ssmd_data_inventory`
- [ ] Agent feedback loop (journal + Linear integration)
- [ ] Rate limiting for agent requests

### Testing & Quality
Ref: `docs/plans/designs/kalshi/11-testing.md`

**Unit & Integration Tests:**
- [x] Rust unit tests (66 passing)
- [x] Go CLI tests
- [ ] Docker compose for local integration testing (NATS, MinIO, Redis)
- [ ] Integration test framework with in-memory middleware

**Replay Testing:**
- [ ] ReplayTest framework (compare baseline vs candidate versions)
- [ ] CLI: `ssmd test replay --feed --date --baseline --candidate`
- [ ] CLI: `ssmd test compare --env-a --env-b --duration`
- [ ] GitHub Actions workflow for replay on PR

**Backtesting:**
- [ ] SimulatedClock for non-realtime testing
- [ ] CLI: `ssmd backtest --feed --date --strategy --speed`
- [ ] Step-through mode for debugging

## Open Questions

Tracked from design documents - decisions needed before implementation.

**From Kalshi Roadmap (`13-roadmap.md`):**
- [ ] Kalshi rate limits - verify API limits for market sync
- [ ] Orderbook depth - full book or top N levels?
- [ ] Historical backfill - does Kalshi provide historical data API?
- [ ] Client auth - API keys sufficient or need more?

**From Agent Pipeline (`2025-12-23-agent-pipeline.md`):**
- [ ] Hot reload - file watcher or explicit reload command for signals?
- [ ] Multi-ticker state - each ticker gets own OrderBook or shared state?
- [ ] Backpressure - what happens if signal evaluation can't keep up?
- [ ] State snapshots - Redis, file journal, or NATS KV for recovery?

## Future Work

### Multicast Feed Recovery
Not needed for initial TCP/WebSocket feeds (Kalshi, Polymarket). Required when adding multicast support.

- [ ] Extend Feed schema with recovery endpoint configuration
- [ ] Snapshot request mechanism (point-in-time state recovery)
- [ ] Replay request mechanism (historical message replay)
- [ ] Recovery source metadata (separate endpoint, different protocol)

### Additional Connectors
- [ ] Polymarket connector
- [ ] Kraken connector (libechidna/C++ integration)

### CLI Enhancements
- [ ] Add `ssmd version` command
- [ ] Add JSON output format (`--output json`)
- [ ] Shell completion scripts (bash/zsh)
- [ ] CI/CD pipeline for automated testing

### Post-Milestone
Ref: `docs/plans/designs/kalshi/13-roadmap.md` Future Work section.

- [ ] TUI admin interface
- [ ] Lua transforms for custom client formats
- [ ] Multi-tenant support
- [ ] Web UI for signal management
- [ ] Signal marketplace (share/import signal definitions)
- [ ] Backtesting framework
