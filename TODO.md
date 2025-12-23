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

## In Progress

_None_

## Pending

### Next: Sequenced Stream Handling
Documented in `docs/plans/2025-12-22-schema-normalization-design.md` TODO section.

- [ ] Add `sequenced: bool` to Protocol struct
- [ ] Add `sequence_field: string` to Protocol struct
- [ ] Sequence number tracking in Rust connector
- [ ] Gap detection and alerting
- [ ] Recovery mechanisms (where protocol supports)

### Future: Multicast Feed Recovery
Not needed for initial TCP/WebSocket feeds (Kalshi, Polymarket). Required when adding multicast support (e.g., market data from exchanges).

- [ ] Extend Feed schema with recovery endpoint configuration
- [ ] Snapshot request mechanism (point-in-time state recovery)
- [ ] Replay request mechanism (historical message replay)
- [ ] Recovery source metadata (separate endpoint, different protocol)

### Metrics & Observability
- [ ] Latency histograms (message receive to write, end-to-end)
- [ ] Prometheus histogram buckets for P50/P95/P99 latency
- [ ] Timestamp tracking at each pipeline stage

### Enhancements (when needed)
- [ ] Add `ssmd version` command
- [ ] Add JSON output format (`--output json`)
- [ ] Shell completion scripts (bash/zsh)
- [ ] CI/CD pipeline for automated testing
