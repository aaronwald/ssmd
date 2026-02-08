# SSMD Secmaster Expert Memory

## 2026-02-07: Multi-Exchange Agent Review

### DB Schema State
- `events`, `markets`, `series`, `series_fees` - Kalshi-only (event_ticker PK)
- `pairs` - Kraken spot + perps (PK: pair_id only — exchange NOT in PK, collision risk)
- `polymarket_conditions` + `polymarket_tokens` - Polymarket conditions/tokens
- No `exchange` column on events/markets/series — assumes Kalshi
- pair_id VARCHAR(128) but originally VARCHAR(32), expanded in migration 0009

### Connector Architecture
- Kraken connector: **SPOT ONLY** (wss://ws.kraken.com/v2), channels: ticker + trade
- Kraken perp data (fundingRate, markPrice, openInterest) comes from REST sync only, NOT live WS
- Kraken futures WS would need separate endpoint: wss://futures.kraken.com/ws/v1
- Symbols from KRAKEN_SYMBOLS env var, default: BTC/USD, ETH/USD

### NATS Topology
- Kalshi: per-category streams (PROD_KALSHI_CRYPTO, etc.), subject: {env}.kalshi.{cat}.json.{type}.{ticker}
- Kraken: single stream (PROD_KRAKEN), subject: {env}.kraken.json.{type}.{symbol}
- Polymarket: single stream (PROD_POLYMARKET), subject: {env}.polymarket.json.{type}.{token_id}
- Kraken WS symbols get sanitized: BTC/USD → BTC-USD in NATS subjects

### Key Gaps Found
- System prompt has zero exchange awareness
- Agent tools have no Kraken/Polymarket data access (no API endpoints exist)
- DQ check hardcoded to Kalshi (ticker prefixes, NATS streams, Kalshi API)
- pair_id PK collision risk with more exchanges (recommend composite PK)
- Perp data overwrites on sync — no historical tracking (recommend pair_snapshots table)

### Patterns
- Each exchange has own DB tables (not shared schema)
- Sync commands are separate: secmaster sync, kraken sync, polymarket sync
- Agent tools use ssmd-data HTTP API, not direct DB access
- SubjectBuilder handles {prefix}.json.{type}.{ticker} pattern
- sanitize_subject_token: "/" → "-", strips ".", ">", "*"
