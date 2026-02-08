# QA/Testing Expert Memory

## 2026-02-06: Polymarket Connector Review

### Codebase Test Patterns
- InMemoryTransport is the standard mock for NATS publish verification
- Writer tests use subscribe→write→assert pattern on InMemoryTransport
- Message parsing tests use const JSON strings with full and minimal field coverage
- Connector tests are mostly unit-level (creation, constants, sharding logic) — no async integration due to real WS dependency
- Kraken tests: 8 message tests + 8 writer tests = ~16 total; Polymarket: 10 message + 9 writer + 7 connector + 4 discovery = ~30 total
- `process::exit(1)` on WS errors is standard pattern — K8s restarts handle reconnection

### Key Test Gaps Found (HIGH priority)
- No negative/error path tests for writer (invalid JSON, truncated messages)
- No test for `BestBidAsk` routing to `json.ticker` subject (only PriceChange tested for ticker)
- No test for `NewMarket` routing to `json.lifecycle` subject (only MarketResolved tested)
- No test for `subscribe_additional()` message format correctness
- Market discovery has no test for closed market filtering or missing field handling
- No test for `connect()` called twice (should error)
- Condition IDs with dots/special chars — sanitization tested in middleware but not end-to-end in writer

### Key Test Gaps Found (MEDIUM priority)
- Empty book snapshot (0 buys, 0 sells) not tested
- Book with null/missing timestamp not tested
- PriceChange with multiple items not tested
- `extract_token_ids` with empty markets not tested
- WebSocket error variants not comprehensively tested
- No test for writer `close()` behavior

### Effective Co-Panelists for Connector Review
- Data Feed Expert: validates protocol semantics
- Security Engineer: input sanitization, NATS injection
- Performance Expert: hot path allocation, cache behavior
