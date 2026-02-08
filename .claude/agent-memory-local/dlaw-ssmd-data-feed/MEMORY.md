# SSMD Data Feed Expert Memory

## 2026-02-06: Polymarket Connector Review

### Key Findings
- `serde(tag = "event_type")` used for Polymarket (reliable) vs `serde(untagged)` for Kraken (fragile). Good pattern choice.
- `subscribe_additional()` uses wrong message format (`operation: subscribe` instead of `type: market`). Dead code but landmine.
- Multi-shard `process::exit(1)` on proactive reconnect kills ALL shards — acceptable for MVP, needs per-shard reconnect for scale.
- `extract_token_ids()` lacks deduplication — flat_map without dedup could waste instrument slots.
- BestBidAsk and PriceChange both routed to `json.ticker` — different schemas on same subject.
- Gamma API discovery has no retry/backoff on pagination failures.
- All shards share single AtomicU64 activity tracker — health check can't detect per-shard staleness.
- `DEFAULT_POLL_INTERVAL_SECS` defined but unused — no periodic re-discovery implemented.
- `polymarket-prod.yaml` uses `type: nats` instead of `transport_type: nats` — may be a deserialization issue.

### Patterns Confirmed
- New exchange modules follow: `mod.rs`, `messages.rs`, `websocket.rs`, `connector.rs`, `writer.rs` — now also `market_discovery.rs` for Polymarket
- Each exchange gets its own Writer impl — PolymarketNatsWriter consistent with Kraken pattern
- SubjectBuilder extended with `json_orderbook()` and `json_lifecycle()` for Polymarket needs
- PONG filtering is dual-layer: connector skips forwarding, writer skips publishing
- condition_id routing (not token_id) correctly groups Yes/No outcomes
- Sharding via `chunks(MAX_INSTRUMENTS_PER_CONNECTION)` with staggered startup + jitter

### Effective Review Patterns
- Always compare new exchange module against Kraken (simpler) and Kalshi (more complex) for consistency
- Check dead code methods for API format correctness — unused != correct
- Multi-shard designs need per-shard health/reconnect consideration
- Always verify feed/environment YAML field names match the metadata deserializer

## 2026-02-07: Operator Generalization Review

### Key Findings
- Operator `constructConfigMap()` reconstructs feed.yaml/env.yaml from hardcoded strings — root cause of all 6 Kalshi-specific issues
- Kraken static deployment (`ssmd-kraken-config` ConfigMap) is the correct pattern: hand-crafted configs, operator just mounts
- `feed-{name}` ConfigMap with `defaults` section already exists for Kalshi, needs extending to Kraken/Polymarket
- Auth env vars (`KALSHI_API_KEY`) hardcoded in operator — needs generic env var injection
- Subscription models fundamentally differ: Kalshi=secmaster+CDC, Kraken=static symbols, Polymarket=Gamma discovery
- `type: nats` vs `transport_type: nats` field name inconsistency persists across env YAMLs
- Operator-generated pods lack securityContext (runAsNonRoot, readOnlyRootFilesystem) that static Kraken has
- Three different ConfigMap naming conventions in use — needs standardization

### Architecture Insight
- Operator should **read** feed ConfigMaps, not **generate** them
- `getFeedDefaults()` already reads from `feed-{name}` ConfigMap for image/version — extend for transport/auth
- Archiver controller is already more generic (reads stream/consumer/filter from CR spec directly)
- The Rust connector binary's `main.rs` match arm routing by feed name is the right abstraction layer — operator doesn't need to know exchange-specific channel logic

### Review Patterns
- Compare operator-generated vs hand-crafted deployments for feature gaps (security context, env vars)
- Check ConfigMap naming conventions for consistency across operator and static deployments
- Validate generated YAML field names against Rust serde deserializer expectations

## 2026-02-07: Kraken REST API Secmaster Research

### API Findings
- `GET /0/public/AssetPairs` returns ALL 1,475 spot pairs in one call (no pagination)
- No spot vs futures filtering needed — this endpoint IS spot-only (futures are at `/0/public/AssetPairs?pair=PI_XBTUSD` or separate API)
- Fields per pair: altname, wsname, base, quote, pair_decimals, lot_decimals, cost_decimals, lot_multiplier, tick_size, ordermin, costmin, status, fees[], fees_maker[], leverage_buy/sell, margin_call/stop, fee_volume_currency
- Status values: online (1424), cancel_only (27), post_only (21), reduce_only (3)
- 633 unique base assets, primary quotes: ZUSD (632), ZEUR (609), USDT (47), USDC (46), XXBT (31)
- Position limits (long_position_limit, short_position_limit) only on 269 of 1475 pairs

### Naming Convention (CRITICAL)
- REST key: `XXBTZUSD` (X-prefix for crypto, Z-prefix for fiat)
- altname: `XBTUSD` (no slashes, legacy naming)
- wsname: `XBT/USD` (slash-separated, used in WS subscriptions)
- WS v2 API accepts both `BTC/USD` and `XBT/USD` (aliased) — our connector uses `BTC/USD`
- Secmaster must store BOTH wsname and altname for cross-referencing REST↔WS

### Rate Limits
- Public endpoints: counter-based, 15-20 max counter by tier, decays 0.33-1/sec
- One AssetPairs call = counter +1, so daily sync is trivial
- No special rate limiting for public endpoints beyond the counter

## 2026-02-08: Data Quality Scoring for Connectors

### How DQ Daily Scores Connectors
- `ssmd dq daily` queries Prometheus metrics for each connector feed
- Metrics used: `ssmd_connector_websocket_connected`, `ssmd_connector_messages_total` (24h increase), `ssmd_connector_idle_seconds`, `ssmd_connector_markets_subscribed`
- NATS JetStream `streams.info()` for binary "has data" check per stream
- Label filters: `feed="kalshi",category="crypto"`, `feed="kraken-futures"`, `feed="polymarket"`

### Connector Metric Requirements
- All connectors MUST expose Prometheus metrics on port 8080 `/metrics` for DQ scoring to work
- If a connector lacks metrics, DQ scores that feed at 0 for Prometheus-sourced checks
- Kraken Futures connector gained metrics in v0.8.8 — verify any new connectors have instrumentation

### Message Flow Baselines (calibrate after 7 days)
- Kalshi crypto: ~10K messages/24h target (scale 0→0, 10000→100)
- Kraken futures: ~1K messages/24h (ticker updates for 2 products)
- Polymarket: ~500 messages/24h (varies with market activity)

### NATS Streams Monitored
- `PROD_KALSHI_CRYPTO`, `PROD_KRAKEN_FUTURES`, `PROD_POLYMARKET`
- DQ check uses Deno NATS client on port 4222 (no need for HTTP monitoring port 8222)

### Architecture Implications for Secmaster
- Single API call gets all pairs — no pagination complexity
- Could sync all 1,475 pairs or filter by status=online (1,424)
- No CDC equivalent — poll-based sync (daily or on-demand) is sufficient
- Quote currency normalization needed: ZUSD→USD, ZEUR→EUR, XXBT→XBT/BTC
- Fee schedule is per-pair with volume tiers — store as JSONB or separate table
