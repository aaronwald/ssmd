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

## Database Changes

### Migration: Add `series` Table

```sql
-- migrations/002_series.sql

CREATE TABLE IF NOT EXISTS series (
    ticker VARCHAR(64) PRIMARY KEY,
    title TEXT NOT NULL,
    category VARCHAR(64) NOT NULL,
    tags TEXT[], -- Array of tags from API
    is_game BOOLEAN NOT NULL DEFAULT false, -- For Sports filtering
    active BOOLEAN NOT NULL DEFAULT true, -- Soft disable
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_series_category ON series(category) WHERE active = true;
CREATE INDEX idx_series_category_game ON series(category, is_game) WHERE active = true;
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

### 4. Temporal Workflows

Update `ssmd-worker` activities:

```typescript
// Current
async function syncSecmaster() {
  await exec("ssmd secmaster sync --active-only");
}

// New
async function syncSecmaster() {
  // Step 1: Sync series metadata (fast, ~2s for all categories)
  await exec("ssmd series sync");

  // Step 2: Sync markets by series
  await exec("ssmd secmaster sync --by-series");
}
```

## Implementation Order

### Phase 1: Database + CLI (no breaking changes)

1. Create migration `002_series.sql`
2. Add `ssmd series sync` command
   - Fetches `/search/tags_by_categories`
   - Fetches `/series?category={cat}` for each category
   - Applies Sports filter (GAME/MATCH pattern)
   - Upserts to `series` table
3. Add `ssmd secmaster sync --by-series` flag
   - Reads series from DB
   - Queries markets by `series_ticker` instead of category
4. Test locally with existing connector

### Phase 2: API + Connector

1. Add `/v1/series` endpoint to ssmd-data-ts
2. Update Rust connector to use series-based queries
3. Test end-to-end

### Phase 3: Temporal + Deploy

1. Update Temporal worker to call new commands
2. Build and tag new CLI image
3. Build and tag new worker image
4. Deploy to cluster

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
| `migrations/002_series.sql` | New file |
| `ssmd-agent/src/cli/commands/series.ts` | New file |
| `ssmd-agent/src/cli/commands/secmaster.ts` | Add `--by-series` flag |
| `ssmd-agent/src/lib/db.ts` | Add series queries |
| `ssmd-agent/src/lib/kalshi.ts` | Add series API calls |
| `ssmd-data-ts/src/routes/series.ts` | New file |
| `ssmd-rust/crates/connector/src/secmaster.rs` | Use series-based queries |
| `varlab/workers/kalshi-temporal/src/activities.ts` | Update sync command |
