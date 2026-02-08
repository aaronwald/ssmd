# Code Reviewer Memory

## 2026-02-07: Multi-Exchange Support Review (Pairs, Polymarket, Snapshots, Routes)

### HIGH Priority Findings
- **URLPattern + colon-namespaced IDs**: Pair IDs use `kraken:XXBTZUSD` format, but routes use `:pairId` pattern param. URLPattern may not handle literal colons in path segments correctly. Need testing.
- **Route ordering fragility**: `/v1/pairs/stats` must be registered before `/v1/pairs/:pairId` to avoid being swallowed. Same pattern exists for series. Works today but fragile.

### MEDIUM Priority Findings
- `softDeleteMissingPairs` and `softDeleteMissingConditions` bypass Drizzle (`getRawSql()` directly) — inconsistent with other functions that take `db: Database`. Existing tech debt.
- `listConditions` explicit select includes `deletedAt` (always null due to WHERE filter) — noise in API response.
- `insertPerpSnapshots` takes `NewPair[]` but creates snapshots internally — pragmatic but param name misleading.

### Patterns Observed in This Codebase
- DB operations: list/get/stats/upsert/softDelete per entity, exported via `mod.ts` barrel
- Drizzle ORM: conditions array + `sql.join` for dynamic WHERE clauses
- Route registration: order-sensitive `route()` calls, literals before params
- Soft delete: `deleted_at` column + `isNull` filter everywhere
- Batch sizes: 500 for Drizzle inserts, 10000 for raw SQL temp table inserts (PG 65534 param limit)
- Agent tools: `z.string().optional().nullable().describe()` pattern for optional Zod fields (LLMs send null)
- Tool grouping: arrays by domain (`calendarTools`, `dataTools`, `secmasterTools`) merged into `allTools`

### Questions Raised
- Migration deployment ordering: migration 0012 namespaces pair_ids, but sync code now writes prefixed IDs — need coordinated deploy
- Snapshot retention: `pair_snapshots` grows ~17K rows/day with no retention policy
- `GET /v1/series/:ticker` route may be missing (agent tool references it but not visible in diff)

### Effective Expert Pairings for This Task Type
- Database Expert: schema review, migration safety, query optimization
- API Designer: route patterns, URL encoding, REST conventions
- Security: no new concerns (auth scope `secmaster:read` correctly applied)
