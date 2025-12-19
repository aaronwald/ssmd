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

## In Progress

_None_

## Pending

### Phase 2: Runtime Layer
- [ ] etcd integration for intraday configuration
- [ ] Operational state storage
- [ ] Trading day lifecycle management
- [ ] Key management with sealed secrets

### Phase 3: Data Collection
- [ ] Kalshi WebSocket connector
- [ ] Normalization pipeline
- [ ] S3 storage integration
- [ ] NATS JetStream publishing

### Enhancements
- [ ] Add `ssmd version` command
- [ ] Add JSON output format (`--output json`)
- [ ] Shell completion scripts (bash/zsh)
- [ ] CI/CD pipeline for automated testing
