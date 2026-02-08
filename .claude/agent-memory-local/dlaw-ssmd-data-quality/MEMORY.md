# SSMD Data Quality Expert Memory

## 2026-02-07: Kraken Secmaster DQ Assessment

### Key Findings
- `pairs` table PK is `pair_id` only — no composite key with `exchange`. Cross-exchange collision risk for exchanges that reuse ticker names (e.g., Binance spot/futures both use `BTCUSDT`).
- `normalizeBase()` X-stripping heuristic can corrupt assets starting with X (e.g., `XAUT` → `AUT`). Fallback path has no logging.
- NATS subjects use `sanitize_subject_token(symbol)` (e.g., `BTC-USD`) but secmaster `ws_name` stores raw Kraken format (`XBT/USD` or `PF_XBTUSD`). No join layer exists.
- No sync health metrics or `last_synced_at` tracking for automated alerting.
- Perp base/quote parsing only covers USD/EUR/GBP — fragile to new quote currencies.
- `ws_name VARCHAR(32)` was not widened in migration 0009 (pair_id was widened to 128).
- Spot and perp upserts don't cross-null each other's fields — stale data possible on type change.

### DQ Check Patterns in Codebase
- `dq-check.ts` validates archived JSONL.gz files (Kalshi backtest data)
- Checks: parse errors, timestamp ordering, field completeness, bid>ask, volume monotonicity, price range
- Pattern: per-file → per-ticker aggregation with issue flagging
- No existing DQ check for secmaster/sync health — recommended to add

### Schema Reference
- Pairs table: `schema.ts:122-161`, PK on `pair_id`
- Pairs migration: `0008` (create), `0009` (extend for spot+perps)
- Spot upsert: `pairs.ts:15-48`
- Perp upsert: `pairs.ts:54-99`
- Soft delete: `pairs.ts:105-134` (scoped by `exchange, market_type`)
- Settings table exists for key-value storage: `schema.ts:90-94`

### Kraken Connector <-> Secmaster Alignment
- Connector WS v2 uses `symbol` field (e.g., `"BTC/USD"`) → sanitized to `BTC-USD` for NATS
- Secmaster uses Kraken REST API key as `pair_id` (e.g., `XXBTZUSD`) and `wsname` field for `ws_name`
- These are different namespaces — need mapping layer for DQ joins

### Kraken Connector Scope (verified 2026-02-07)
- Connector is **spot only**: `wss://ws.kraken.com/v2`, channels: `ticker` + `trade`
- Futures WS is a separate API: `wss://futures.kraken.com/ws/v1` — not implemented
- Symbols from `KRAKEN_SYMBOLS` env var, defaults to `["BTC/USD", "ETH/USD"]`
- Perp data is REST-sync only (no live stream), so no NATS DQ reconciliation needed for perps
- Connector metrics labeled `("kraken", "spot")` explicitly

### PK Collision Prevention Approaches Evaluated
- Option A: Composite PK `(pair_id, exchange)` — high blast radius (FK, ON CONFLICT, temp tables)
- Option B: Namespaced PK `"kraken:XXBTZUSD"` — recommended, zero schema change
- Option C: Surrogate PK + composite unique — over-engineered for this use case

### Time-Series Perp Data Pattern
- Separate `pair_snapshots` table recommended for time-varying fields (markPrice, fundingRate, etc.)
- `pairs` table keeps static metadata only (contractSize, marginLevels, etc.)
- At 50 contracts * 15min intervals = ~150K rows/month — no partitioning needed initially
- Don't forget to simplify `updated_at` trigger after moving time-varying fields out

### Effective Co-Panelists for This Task Type
- Database Expert: for PK collision analysis and schema adequacy
- Security Engineer: for data integrity concerns
- Senior Developer: for normalization heuristic review

## 2026-02-08: Daily Data Quality Scoring System

### Architecture
- `ssmd dq daily` CLI command scores all Phase 1 feeds daily
- Data sources: Prometheus (connector metrics), PostgreSQL (funding snapshots + score persistence), NATS JetStream (stream health)
- Temporal workflow `dataQualityWorkflow` runs daily at 06:00 UTC via `ssmd-data-quality-daily` schedule
- Results sent to ntfy topic `ssmd-data-quality` (subscribe at `https://ntfy.varshtat.com/ssmd-data-quality`)

### Scoring
- Composite formula: `(kalshi × 0.35) + (kraken × 0.30) + (polymarket × 0.15) + (funding × 0.20)`
- Per-feed scores 0-100 based on: WS connected, message flow, idle time, markets subscribed, stream has data
- Funding rate: consumer connected, snapshot recency, daily count, products present, flush rate
- Grades: GREEN (>=85), YELLOW (60-84), RED (<60)
- Hard RED overrides: any WS disconnected, funding >1h stale, zero messages on Kalshi/Kraken

### Graceful Degradation
- If Prometheus unreachable, all Prometheus-sourced metrics cap at 50
- NATS + PostgreSQL checks still work independently
- `prometheusDegraded` flag in output indicates this state

### Key Files
- CLI: `ssmd/ssmd-agent/src/cli/commands/dq.ts` — `runDailyDqCheck()` function (~200 lines)
- Migration: `ssmd/ssmd-agent/migrations/0014_create_dq_daily_scores.sql`
- Temporal activity: `varlab/workers/kalshi-temporal/src/activities.ts` — `runDataQualityCheck()`
- Temporal workflow: `varlab/workers/kalshi-temporal/src/workflows.ts` — `dataQualityWorkflow()`
- Network policies: ssmd egress + Prometheus ingress allow port 9090

### Interacting with the System
- Run manually: `ssmd dq daily` (human-readable) or `ssmd dq daily --json` (structured)
- JSON schema: `{ date, feeds: { "kalshi-crypto": { score, messages, idleSec, markets, ... }, ... }, composite, grade, issues, prometheusDegraded }`
- Scores persisted to `dq_daily_scores` table with JSONB `details` column
- Query historical scores: `SELECT * FROM dq_daily_scores WHERE check_date >= '2026-02-01' ORDER BY check_date, feed`
- Environment vars: `PROMETHEUS_URL`, `NATS_URL`, `DATABASE_URL`

### Threshold Calibration
- Initial thresholds are estimates; after 7 days of data, review raw values in `details` JSONB and adjust
- Message flow scales: Kalshi 10K, Kraken 1K, Polymarket 500 (per 24h)
- Market counts: Kalshi 50 max, Kraken 2 max
