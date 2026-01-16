# Series-Based Sync Implementation Plan

## Overview

Replace the slow time-based market sync (`close_within_hours=48`) with a targeted series-based approach. This reduces sync time from minutes to seconds by only fetching markets for specific series we care about.

## Key Design Decisions

1. **ONE series table** stores all series metadata from Kalshi
2. **Two filtering APIs** determine which series to track:
   - `/search/tags_by_categories` - tags per category
   - `/search/filters_by_sport` - sports-specific scopes (Games vs Futures)
3. **Sports filtering**: Include only "Games" scope (tickers ending in GAME/MATCH)
4. **Other categories**: Include all series (no filtering needed)
5. **Uniform market queries**: Once we have series, `series_ticker={ticker}&status=open` works the same for all
6. **Tag-based Temporal jobs**: Jobs are configurable by tag for horizontal scaling

## Database Changes

### Migration: Add `series` Table

```sql
-- migrations/002_series.sql

CREATE TABLE IF NOT EXISTS series (
    ticker VARCHAR(64) PRIMARY KEY,
    title TEXT NOT NULL,
    category VARCHAR(64) NOT NULL,
    tags TEXT[], -- Array of tags from API (e.g., ["Basketball", "Pro Basketball"])
    is_game BOOLEAN NOT NULL DEFAULT false, -- For Sports: GAME/MATCH in ticker
    active BOOLEAN NOT NULL DEFAULT true, -- Soft disable
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Query by category
CREATE INDEX idx_series_category ON series(category) WHERE active = true;

-- Query by category + games filter (for Sports)
CREATE INDEX idx_series_category_game ON series(category, is_game) WHERE active = true;

-- Query by tag (for Temporal job filtering)
CREATE INDEX idx_series_tags ON series USING GIN(tags) WHERE active = true;
```

**Tag query example:**
```sql
-- Get all series for Basketball tag (Sports games only)
SELECT * FROM series
WHERE 'Basketball' = ANY(tags)
  AND is_game = true
  AND active = true;

-- Get all series for Economics tag (all series)
SELECT * FROM series
WHERE 'Economics' = ANY(tags)
  AND active = true;
```

## Component Changes

### 1. Secmaster Sync (CLI / Temporal)

**Current flow:**
```
GET /markets?close_within_hours=48  → 3600+ markets, rate limited
```

**New flow:**
```
1. Fetch category metadata:
   GET /search/tags_by_categories
   GET /search/filters_by_sport

2. For each category we track:
   GET /series?category={cat}
   → Filter: Sports keeps only GAME/MATCH patterns
   → Upsert to series table

3. For each active series in DB:
   GET /markets?series_ticker={ticker}&status=open
   GET /markets?series_ticker={ticker}&status=closed&min_close_ts={24h}
   GET /markets?series_ticker={ticker}&status=settled&min_settled_ts={24h}
```

**CLI command changes:**

```bash
# New command: sync series metadata
ssmd series sync

# Updated command: sync markets by series
ssmd secmaster sync --by-series
```

### 2. ssmd-data-ts API

Add endpoints for connector to query series:

```
GET /v1/series?category={cat}
GET /v1/series?category={cat}&is_game=true
```

Response:
```json
{
  "series": [
    {"ticker": "KXNBAGAME", "title": "Professional Basketball Game", "category": "Sports"}
  ]
}
```

### 3. Rust Connector

**Current flow:**
```rust
// Queries secmaster DB for markets by category
let markets = db.get_markets_by_category(&category)?;
```

**New flow:**
```rust
// 1. Query series for this connector's category
let series = api.get_series(&category).await?;

// 2. For each series, get open markets
for s in series {
    let markets = api.get_markets_by_series(&s.ticker, "open").await?;
    // Subscribe to each market
}
```

### 4. Temporal Workflows (Tag-Based Configuration)

**Design goal**: Enable horizontal scaling by running separate jobs per tag.

**Current**: One monolithic sync job for everything.

**Future**: Tag-based jobs that can be added/removed via configuration.

```typescript
// Workflow input - configurable per schedule
interface SyncWorkflowInput {
  tags: string[];        // e.g., ["Basketball"], ["Soccer"], or ["Economics", "Financials"]
  gamesOnly?: boolean;   // For Sports tags, filter to GAME/MATCH series
}

// Activity: sync series for specific tags
async function syncSeriesForTags(input: SyncWorkflowInput) {
  const tagArgs = input.tags.map(t => `--tag=${t}`).join(" ");
  const gamesFlag = input.gamesOnly ? "--games-only" : "";
  await exec(`ssmd series sync ${tagArgs} ${gamesFlag}`);
}

// Activity: sync markets for series matching tags
async function syncMarketsForTags(input: SyncWorkflowInput) {
  const tagArgs = input.tags.map(t => `--tag=${t}`).join(" ");
  await exec(`ssmd secmaster sync --by-series ${tagArgs}`);
}
```

**CLI commands support tag filtering:**

```bash
# Sync series for specific tags
ssmd series sync --tag=Basketball --tag=Hockey --games-only

# Sync markets for series matching tags
ssmd secmaster sync --by-series --tag=Basketball --tag=Hockey

# Sync all (default behavior)
ssmd series sync
ssmd secmaster sync --by-series
```

**Example Temporal schedules (future scaling):**

| Schedule ID | Tags | Interval | Notes |
|-------------|------|----------|-------|
| `sync-sports-us` | Basketball, Football, Hockey, Baseball | 5m | US pro sports |
| `sync-sports-soccer` | Soccer | 5m | European leagues |
| `sync-financials` | Financials | 15m | S&P, Nasdaq |
| `sync-politics` | Politics, Elections | 30m | Less time-sensitive |

**Initial deployment**: Single job with all tags, split later as needed.

## Detailed Implementation TODOs

### Phase 1: Database + CLI

#### 1.1 Create Migration
- [x] Create `migrations/002_series.sql`:
```sql
CREATE TABLE IF NOT EXISTS series (
    ticker VARCHAR(64) PRIMARY KEY,
    title TEXT NOT NULL,
    category VARCHAR(64) NOT NULL,
    tags TEXT[],
    is_game BOOLEAN NOT NULL DEFAULT false,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_series_category ON series(category) WHERE active = true;
CREATE INDEX idx_series_category_game ON series(category, is_game) WHERE active = true;
CREATE INDEX idx_series_tags ON series USING GIN(tags) WHERE active = true;
```
- [ ] Run migration: `psql $DATABASE_URL -f migrations/002_series.sql`

#### 1.2 Add Kalshi API Functions
- [x] Edit `ssmd-agent/src/lib/kalshi.ts`, add:
```typescript
export async function fetchTagsByCategories(): Promise<Record<string, string[]>>
export async function fetchFiltersBySport(): Promise<SportFilters>
export async function fetchSeriesByCategory(category: string, tag?: string): Promise<Series[]>
export async function fetchMarketsBySeries(seriesTicker: string, status: string): Promise<Market[]>
```

#### 1.3 Add Database Functions
- [x] Edit `ssmd-agent/src/lib/db.ts`, add:
```typescript
export async function upsertSeries(series: Series[]): Promise<void>
export async function getSeriesByTags(tags: string[], gamesOnly?: boolean): Promise<Series[]>
export async function getAllActiveSeries(): Promise<Series[]>
```

#### 1.4 Create Series Command
- [x] Create `ssmd-agent/src/cli/commands/series.ts`:
```typescript
// ssmd series sync [--tag=X]... [--games-only]
// 1. Fetch tags_by_categories from Kalshi
// 2. For each tag (or all if none specified):
//    - Fetch /series?category={cat}&tags={tag}
//    - Set is_game = ticker.includes("GAME") || ticker.includes("MATCH")
//    - Upsert to DB
```
- [x] Register command in `ssmd-agent/src/cli/main.ts`

#### 1.5 Update Secmaster Command
- [x] Edit `ssmd-agent/src/cli/commands/secmaster.ts`, add flags:
  - `--by-series`: Use series-based sync instead of category-based
  - `--tag=X`: Filter to specific tags (repeatable)
- [x] Implement series-based sync:
```typescript
// For each series (filtered by tags if specified):
//   1. GET /markets?series_ticker={ticker}&status=open → upsert
//   2. GET /markets?series_ticker={ticker}&status=closed&min_close_ts={24h} → update
//   3. GET /markets?series_ticker={ticker}&status=settled&min_settled_ts={24h} → update
```

#### 1.6 Test Locally
- [ ] `ssmd series sync` - sync all series
- [ ] `ssmd series sync --tag=Basketball --games-only` - sync Basketball games
- [ ] `ssmd secmaster sync --by-series` - sync markets by series
- [ ] `ssmd secmaster sync --by-series --tag=Basketball` - sync Basketball markets

### Phase 2: API + Connector

#### 2.1 Add Series API Endpoint
- [ ] Create `ssmd-data-ts/src/routes/series.ts`:
```typescript
// GET /v1/series?tag=X&games_only=true
// Returns: { series: [{ ticker, title, category, tags, is_game }] }
```
- [ ] Register route in `ssmd-data-ts/src/index.ts`

#### 2.2 Update Rust Connector
- [ ] Edit `ssmd-rust/crates/connector/src/secmaster.rs`:
  - Add `get_series(category: &str) -> Vec<Series>` function
  - Change market fetch to use `series_ticker` parameter
  - Remove category-based market fetch

#### 2.3 Test End-to-End
- [ ] Deploy updated ssmd-data-ts
- [ ] Run connector locally against cluster DB
- [ ] Verify markets are fetched correctly

### Phase 3: Temporal + Deploy

#### 3.1 Update Temporal Worker
- [ ] Edit `varlab/workers/kalshi-temporal/src/activities.ts`:
```typescript
export async function syncSecmaster(tags?: string[]) {
  const tagArgs = tags?.map(t => `--tag=${t}`).join(" ") || "";
  await exec(`ssmd series sync ${tagArgs}`);
  await exec(`ssmd secmaster sync --by-series ${tagArgs}`);
}
```
- [ ] Edit `varlab/workers/kalshi-temporal/src/workflows.ts`:
```typescript
interface SyncInput { tags?: string[]; }
```

#### 3.2 Build and Deploy
- [ ] Tag and push CLI: `git tag cli-ts-v0.2.19 && git push origin cli-ts-v0.2.19`
- [ ] Wait for CLI build: `gh run watch`
- [ ] Update worker Dockerfile to use new CLI version
- [ ] Tag and push worker: build new ssmd-worker image
- [ ] Update `varlab/clusters/homelab/apps/ssmd/worker/deployment.yaml`
- [ ] Push varlab changes, wait for Flux reconcile

### Phase 4: Admin Dashboard + Cleanup

#### 4.1 Add Series Stats to API
- [ ] Add `/v1/stats/series` endpoint showing markets per series
- [ ] Add series filter to existing market stats

#### 4.2 Update Admin Dashboard
- [ ] Update ssmd-admin.varshtat.com to show series breakdown
- [ ] Add series filter to market views

#### 4.3 Clean Old Data
- [ ] Delete stale markets/events from before series-based sync
- [ ] Truncate and resync with new approach

#### 4.4 Expose Series to REPL
- [ ] Add series tools to LangGraph agent
- [ ] Enable querying series stats from REPL

### Verification Checklist
- [ ] `ssmd series sync` populates series table
- [ ] `ssmd secmaster sync --by-series` fetches markets by series
- [ ] Temporal job runs successfully
- [ ] Connector receives correct markets from API
- [ ] NBA/NFL/NHL games appear in secmaster within 5 minutes of sync
- [ ] Admin dashboard shows series breakdown

## Series Filtering Logic

```typescript
function shouldTrackSeries(series: Series, category: string): boolean {
  if (category === "Sports") {
    // Only track game series for Sports
    const ticker = series.ticker.toUpperCase();
    return ticker.includes("GAME") || ticker.includes("MATCH");
  }
  // Track all series for other categories
  return true;
}
```

## Categories and Expected Series Count

| Category | Total Series | Tracked Series |
|----------|--------------|----------------|
| Economics | 405 | 405 (all) |
| Elections | 537 | 537 (all) |
| Entertainment | 2,181 | 2,181 (all) |
| Financials | 170 | 170 (all) |
| Politics | 2,689 | 2,689 (all) |
| Sports | 1,061 | ~100 (games only) |

## Performance Expectations

| Operation | Expected Time |
|-----------|---------------|
| `/search/tags_by_categories` | ~200ms |
| `/series?category=Sports` | ~200ms |
| Series upsert (100 rows) | ~50ms |
| `/markets?series_ticker=X&status=open` | ~150ms |
| Full series sync (all categories) | ~5s |
| Full market sync (by series) | ~30s |

## Rollback Plan

If series-based sync has issues:
1. Keep old `--active-only` flag working
2. Can revert to time-based sync by removing `--by-series` flag
3. Series table doesn't affect existing functionality

## Files to Modify

| File | Changes |
|------|---------|
| `migrations/002_series.sql` | New file: series table with GIN index |
| `ssmd-agent/src/cli/commands/series.ts` | New file: `ssmd series sync --tag=X --games-only` |
| `ssmd-agent/src/cli/commands/secmaster.ts` | Add `--by-series --tag=X` flags |
| `ssmd-agent/src/lib/db.ts` | Add series queries (by tag, by category) |
| `ssmd-agent/src/lib/kalshi.ts` | Add `tags_by_categories`, `filters_by_sport`, series API calls |
| `ssmd-data-ts/src/routes/series.ts` | New file: `/v1/series?tag=X&games_only=true` |
| `ssmd-rust/crates/connector/src/secmaster.rs` | Use series-based queries |
| `varlab/workers/kalshi-temporal/src/workflows.ts` | Add tag-based workflow input |
| `varlab/workers/kalshi-temporal/src/activities.ts` | Update sync commands with tag args |

## Future Scaling Path

When a single sync job becomes too slow or we need different intervals per category:

1. **Split by domain**: Create separate Temporal schedules for Sports, Financials, Politics
2. **Split by sport**: Separate schedules for Basketball, Soccer, Hockey
3. **Adjust intervals**: Sports every 5m, Politics every 30m
4. **Add/remove tags**: Just update schedule input, no code changes needed

The tag-based CLI design makes all of this configuration, not code.
