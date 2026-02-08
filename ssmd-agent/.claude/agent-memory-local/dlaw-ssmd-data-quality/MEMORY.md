# SSMD Data Quality Expert Memory

## 2026-02-07: Multi-Exchange DQ Extension Review

### Key Findings
- Polymarket `last_trade_price` has NO `trade_id` — cannot do Kalshi-style reconciliation
- Polymarket DQ should be NATS-only (count, gap, duplicate detection) for v1
- Kalshi DQ code uses `PROD_KALSHI_{CATEGORY}` but connector publishes to `PROD_KALSHI` (single stream) — mismatch
- Kraken has `trade_id` in both WS and REST, but NATS envelope wraps trades in `data[]` array
- Kraken REST `/0/public/Trades` uses nonce-based pagination (not timestamp), complicates time-window queries
- Polymarket NATS subject uses `event_type` not `trade` — correct pattern: `prod.polymarket.json.last_trade_price.{condition_id}`
- `--count 10000` NATS fetch limit could silently truncate for high-volume tickers

### Exchange NATS Patterns
| Exchange | Stream | Subject Pattern | Trade Match Key |
|----------|--------|----------------|-----------------|
| Kalshi | `PROD_KALSHI` | `prod.kalshi.{cat}.json.trade.{ticker}` | `trade_id` |
| Kraken | `PROD_KRAKEN` | `prod.kraken.json.trade.{pair}` (pair: BTC-USD) | `trade_id` |
| Polymarket | `PROD_POLYMARKET` | `prod.polymarket.json.last_trade_price.{condition_id}` | composite (no trade_id) |

### NATS Message Formats
- Kalshi: `{ type: "trade", msg: { trade_id, market_ticker, ... } }`
- Kraken: `{ channel: "trade", type: "update", data: [{ trade_id, symbol, price, qty, ... }] }`
- Polymarket: `{ event_type: "last_trade_price", asset_id, market, price, side, size, ... }`

### Reconciliation Strategy per Exchange
- **Kalshi**: Full API reconciliation via `KalshiClient.fetchAllTrades()` (existing)
- **Kraken**: API reconciliation via `GET /0/public/Trades?pair=X&since=Y` (needs client)
- **Polymarket**: NATS-only checks (no reliable trade REST API)

### DQ Secmaster Checks (prioritized)
1. Pair count stability (delta > 10% = alert)
2. Namespace consistency (`{exchange}:` prefix)
3. Stale data detection (`updated_at > 24h` for active pairs)
4. Base/quote normalization (XBT vs BTC)
5. Deleted pair audit (soft-deleted but still referenced)

### Patterns Observed
- Feed YAML has `stream` and `subjectPrefix` in defaults — should be DQ source of truth
- `inferCategory()` is Kalshi-specific, brittle, and shouldn't be generalized
- No TypeScript feed config loader exists yet — would benefit DQ and other CLI commands
- Only `KalshiClient` exists in `src/lib/api/` — no Kraken or Polymarket clients

### Files Reference
- DQ code: `ssmd-agent/src/cli/commands/dq.ts`
- Kalshi API client: `ssmd-agent/src/lib/api/kalshi.ts`
- Kraken writer: `ssmd-rust/crates/connector/src/kraken/writer.rs`
- Polymarket writer: `ssmd-rust/crates/connector/src/polymarket/writer.rs`
- Feed configs: `exchanges/feeds/{kalshi,kraken,polymarket}.yaml`
- DB schema: `ssmd-agent/src/lib/db/schema.ts`

## 2026-02-08: Daily DQ Scoring System Implemented

### New CLI Command: `ssmd dq daily`
- Scores all Phase 1 feeds: Kalshi Crypto, Kraken Futures, Polymarket, Funding Rate
- Data sources: Prometheus (connector metrics), PostgreSQL (funding snapshots), NATS JetStream (stream health)
- `--json` flag outputs structured JSON for machine consumption
- Persists scores to `dq_daily_scores` table (migration 0014)

### Composite Scoring
- Formula: `(kalshi × 0.35) + (kraken × 0.30) + (polymarket × 0.15) + (funding × 0.20)`
- Grades: GREEN (>=85), YELLOW (60-84), RED (<60)
- Hard RED overrides for critical failures (WS disconnect, zero messages, stale funding)

### Integration Points
- Temporal: `dataQualityWorkflow` runs `ssmd dq daily --json`, formats ntfy message
- ntfy: Topic `ssmd-data-quality` (separate from `ssmd-secmaster`)
- Network policies updated for Prometheus access from ssmd namespace
- `sendNotification` now supports per-topic routing

### Environment Variables
- `PROMETHEUS_URL` — defaults to `http://kube-prometheus-stack-prometheus.observability.svc:9090`
- `NATS_URL` — defaults to `nats://nats.nats.svc:4222`
- `DATABASE_URL` — required

### Graceful Degradation
- Prometheus unreachable → scores capped at 50, `prometheusDegraded: true`
- NATS unreachable → exits with error (required for stream checks)

### Future Calibration
- Initial message flow thresholds are estimates (Kalshi 10K, Kraken 1K, Polymarket 500)
- After 7 days, review `details` JSONB in `dq_daily_scores` to calibrate
