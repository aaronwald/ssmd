# SSMD Secmaster Expert Memory

## Key Patterns

### Data Model (Feb 2026)
- **Kalshi**: events -> markets (PK: ticker), series (PK: ticker), series_fees
- **Kraken**: pairs (PK: pair_id, namespaced as `exchange:symbol`), pair_snapshots (time-series)
- **Polymarket**: polymarket_conditions (PK: condition_id), polymarket_tokens (PK: token_id)
- Pairs table has 30+ columns; perpetual-specific fields (fundingRate, markPrice, etc.) coexist with spot fields
- `active` (boolean) vs `status` (varchar) ambiguity on polymarket_conditions — both exist

### Agent Tools Pattern
- Tools in `src/agent/tools.ts` use `@langchain/core/tools` with Zod schemas
- `apiRequest<T>(path)` hits ssmd-data API with X-API-Key header, 10s timeout
- Existing naming: `list_X` / `get_X` (markets, events, series)
- Export arrays: `calendarTools`, `dataTools`, `secmasterTools`, `allTools`
- Full `select().*` returned from API — no server-side projection
- `encodeURIComponent` used for path params (see getMarket, line 332)

### Token Efficiency
- Pair responses are token-heavy (30+ fields, JSONB blobs for feeSchedule/marginLevels)
- Tool responses should project fields for list operations, full for get-by-id
- Snapshot responses should omit id/pairId (redundant), include analytical fields only

### API Routes
- All secmaster endpoints require `secmaster:read` scope
- Unified stats: `/v1/secmaster/stats` aggregates events + markets + pairs + conditions
- Pair snapshots: `/v1/pairs/:pairId/snapshots` with from/to/limit params
- Route registration order matters — `/v1/pairs/stats` before `/v1/pairs/:pairId`

## 2026-02-07: Multi-Exchange Agent Tools Review

### Task
Reviewed proposed LangGraph agent tools for pairs (Kraken) and conditions (Polymarket).

### HIGH Findings
1. **Token budget**: list_pairs must project ~12 fields, not full 30+ column object
2. **Snapshot tool missing**: get_pair_snapshots is critical for funding rate analysis — recommended as 5th tool

### MEDIUM Findings
3. **Stats tool**: get_secmaster_stats (unified endpoint) recommended as 6th tool
4. **Tool descriptions**: must name exchange (Kraken/Polymarket) for agent routing
5. **URL encoding**: pair_id contains `:` — must encodeURIComponent in path

### Recommendations Given
- 6 tools total: list_pairs, get_pair, get_pair_snapshots, list_conditions, get_condition, get_secmaster_stats
- Organize into pairTools/conditionTools arrays merged into secmasterTools
- Default snapshot `from` to last 24h to avoid unbounded table scans

### Open Questions Raised
- Snapshot retention policy (table growth)
- More exchanges planned? (affects namespacing convention docs)
- Inline token prices in list_conditions? (N+1 query concern)
