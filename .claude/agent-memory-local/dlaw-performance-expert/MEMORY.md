# Performance Expert Memory

## Codebase Patterns

### ssmd Connector Architecture
- Pipeline: Connector (WS receiver) → mpsc channel → Runner → Writer (NATS publisher)
- `Message` struct: `{ tsc: u64, feed: String, data: Vec<u8> }` — raw bytes, no parsing in Runner
- Channel capacity: 1000 for Kalshi and Kraken connectors
- Reconnection strategy: `process::exit(1)` — relies on K8s restart (no in-process retry)
- `Ordering::SeqCst` used everywhere for atomics — codebase-wide pattern, not per-connector

### Known Performance Patterns
- `SubjectBuilder` has `DashMap` cache for trade/ticker subjects BUT `json_trade()`/`json_ticker()` don't use cache — they `format!()` every call. Comment says "acceptable for MVP volume"
- `#[serde(untagged)]` used for WebSocket message parsing — expensive due to backtracking
- Double deserialization: connector parses for filtering, writer re-parses for NATS routing
- `Bytes::from(msg.data.clone())` in writers — extra copy on every publish

### NATS Streams
- Kalshi streams: 512MB-1GB, SECMASTER_CDC has 15min maxAge
- Kraken stream: 256MB, no maxAge set
- All use "prod.{feed}.*" subject patterns

### Polymarket-Specific
- Polymarket uses `#[serde(tag = "event_type")]` (internally tagged) — much better than Kraken's `#[serde(untagged)]` — no backtracking
- Book snapshots can be up to 2 MiB with large `buys`/`sells` arrays — full deser for routing wastes memory
- Channel capacity: 2000 (vs 1000 for Kraken/Kalshi) — justified for multi-shard reconnect bursts
- Market discovery via Gamma REST API: paginated fetch of all active markets at startup
- Proactive reconnect: `process::exit(1)` after 15 min — kills ALL shards, not just stale one
- Condition IDs (hex like `0x1234abcd`) pass through sanitization unchanged — wasted allocation

### Recurring Cross-Connector Issues (tech debt)
- `Bytes::from(msg.data.clone())` in all writers — extra copy, worst for large messages
- `json_*()` subject methods uncached — `format!()` every call
- `sanitize_subject_token()` allocates even for already-valid inputs
- `SystemTime::now()` in activity trackers vs `quanta` TSC clock elsewhere
- `Ordering::SeqCst` everywhere — not needed for simple counters/timestamps

## Session Log

### 2026-02-06: Polymarket Connector Review
- **Task**: Review Polymarket CLOB connector performance (messages, websocket, connector, writer, market_discovery, main.rs)
- **MEDIUM findings**: Full deser of book snapshots for routing, uncached sanitize/subjects, `data.clone()`, Gamma API all-in-memory, SystemTime syscalls
- **LOW findings**: Channel sizing OK, proactive reconnect kills all shards, SeqCst, ping allocation
- **Key insight**: Polymarket's `#[serde(tag)]` is better than Kraken's `#[serde(untagged)]`, BUT book snapshot deser is worse due to large nested arrays
- **Recommendation**: RoutingHeader struct is highest-value optimization
- **Paired with**: Data Feed, Security, QA experts (ssmdstorm team)

### 2026-02-06: Kraken Connector Review
- **Task**: Review Kraken connector performance
- **HIGH findings**: Double JSON deserialization, `#[serde(untagged)]` overhead
- **MEDIUM findings**: `Bytes::from(clone())`, no reconnection backoff, `SeqCst` atomics, per-call symbol sanitization
- **Paired with**: Data Feed, Security, Operations experts (ssmdstorm team)
