# Drizzle ORM Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace raw postgres.js SQL queries with Drizzle ORM for type-safe database access.

**Architecture:** Drizzle wraps existing postgres.js driver. Schema defined in TypeScript, queries use Drizzle's fluent API. Bulk upserts use `onConflictDoUpdate` with explicit `excluded` references.

**Tech Stack:** Deno, drizzle-orm, drizzle-kit, postgres.js

---

## Task 1: Add Drizzle Dependencies

**Files:**
- Modify: `ssmd-agent/deno.json`

**Step 1: Add drizzle-orm dependency**

Add to deno.json imports:
```json
{
  "imports": {
    "drizzle-orm": "npm:drizzle-orm@^0.38.0",
    "drizzle-orm/": "npm:drizzle-orm@^0.38.0/"
  }
}
```

**Step 2: Verify import works**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno eval "import { drizzle } from 'drizzle-orm/postgres-js'; console.log('OK')"
```
Expected: `OK`

**Step 3: Commit**

```bash
git add ssmd-agent/deno.json ssmd-agent/deno.lock
git commit -m "chore: add drizzle-orm dependency"
```

---

## Task 2: Create Drizzle Schema

**Files:**
- Create: `ssmd-agent/src/lib/db/schema.ts`

**Step 1: Create schema file with all tables**

```typescript
/**
 * Drizzle ORM schema definitions
 * Generated from existing PostgreSQL tables, then cleaned up
 */
import {
  pgTable,
  pgEnum,
  varchar,
  text,
  boolean,
  timestamp,
  integer,
  bigint,
  serial,
  numeric,
} from "drizzle-orm/pg-core";

// Fee type enum matching PostgreSQL
export const feeTypeEnum = pgEnum("fee_type", [
  "quadratic",
  "quadratic_with_maker_fees",
  "flat",
]);

// Events table
export const events = pgTable("events", {
  eventTicker: varchar("event_ticker", { length: 64 }).primaryKey(),
  title: text("title").notNull(),
  category: varchar("category", { length: 64 }).notNull().default(""),
  seriesTicker: varchar("series_ticker", { length: 64 }).notNull().default(""),
  strikeDate: timestamp("strike_date", { withTimezone: true }),
  mutuallyExclusive: boolean("mutually_exclusive").notNull().default(false),
  status: varchar("status", { length: 16 }).notNull().default("open"),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
  deletedAt: timestamp("deleted_at", { withTimezone: true }),
});

// Markets table
export const markets = pgTable("markets", {
  ticker: varchar("ticker", { length: 64 }).primaryKey(),
  eventTicker: varchar("event_ticker", { length: 64 }).notNull()
    .references(() => events.eventTicker),
  title: text("title").notNull(),
  status: varchar("status", { length: 16 }).notNull().default("open"),
  closeTime: timestamp("close_time", { withTimezone: true }),
  yesBid: integer("yes_bid"),
  yesAsk: integer("yes_ask"),
  noBid: integer("no_bid"),
  noAsk: integer("no_ask"),
  lastPrice: integer("last_price"),
  volume: bigint("volume", { mode: "number" }),
  volume24h: bigint("volume_24h", { mode: "number" }),
  openInterest: bigint("open_interest", { mode: "number" }),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
  deletedAt: timestamp("deleted_at", { withTimezone: true }),
});

// Series fees table (exclusion constraint lives in SQL migration)
export const seriesFees = pgTable("series_fees", {
  id: serial("id").primaryKey(),
  seriesTicker: varchar("series_ticker", { length: 64 }).notNull(),
  feeType: feeTypeEnum("fee_type").notNull(),
  feeMultiplier: numeric("fee_multiplier", { precision: 6, scale: 4 }).notNull().default("1.0"),
  effectiveFrom: timestamp("effective_from", { withTimezone: true }).notNull(),
  effectiveTo: timestamp("effective_to", { withTimezone: true }),
  sourceId: varchar("source_id", { length: 128 }),
  createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
});

// Inferred types for select/insert
export type Event = typeof events.$inferSelect;
export type NewEvent = typeof events.$inferInsert;
export type Market = typeof markets.$inferSelect;
export type NewMarket = typeof markets.$inferInsert;
export type SeriesFee = typeof seriesFees.$inferSelect;
export type NewSeriesFee = typeof seriesFees.$inferInsert;
```

**Step 2: Verify schema compiles**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/lib/db/schema.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/lib/db/schema.ts
git commit -m "feat(db): add Drizzle schema definitions"
```

---

## Task 3: Update Database Client

**Files:**
- Modify: `ssmd-agent/src/lib/db/client.ts`

**Step 1: Replace client with Drizzle wrapper**

```typescript
/**
 * PostgreSQL database client using Drizzle ORM over postgres.js
 */
import { drizzle } from "drizzle-orm/postgres-js";
import postgres from "postgres";
import * as schema from "./schema.ts";

export type Database = ReturnType<typeof drizzle<typeof schema>>;

let db: Database | null = null;
let sql: ReturnType<typeof postgres> | null = null;

/**
 * Get the Drizzle database instance.
 * Creates connection pool on first call.
 */
export function getDb(): Database {
  if (!db) {
    const url = Deno.env.get("DATABASE_URL");
    if (!url) {
      throw new Error("DATABASE_URL environment variable not set");
    }
    sql = postgres(url, {
      max: 10,
      idle_timeout: 30,
      connect_timeout: 10,
    });
    db = drizzle(sql, { schema });
  }
  return db;
}

/**
 * Get the raw postgres.js client for edge cases.
 * Prefer using getDb() for most queries.
 */
export function getRawSql(): ReturnType<typeof postgres> {
  if (!sql) {
    getDb(); // Initialize if needed
  }
  return sql!;
}

/**
 * Close the database connection pool.
 * Call this before shutting down.
 */
export async function closeDb(): Promise<void> {
  if (sql) {
    await sql.end();
    sql = null;
    db = null;
  }
}
```

**Step 2: Verify client compiles**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/lib/db/client.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/lib/db/client.ts
git commit -m "feat(db): update client to use Drizzle ORM"
```

---

## Task 4: Migrate Fees Module

**Files:**
- Modify: `ssmd-agent/src/lib/db/fees.ts`

**Step 1: Rewrite fees.ts with Drizzle queries**

```typescript
/**
 * Fee database operations using Drizzle ORM
 */
import { eq, isNull, desc, and, lte, or, gte, sql } from "drizzle-orm";
import type { Database } from "./client.ts";
import { seriesFees, type SeriesFee, type NewSeriesFee } from "./schema.ts";

/**
 * Upsert fee changes, skipping duplicates by source_id.
 * Returns count of inserted records.
 */
export async function upsertFeeChanges(
  db: Database,
  changes: NewSeriesFee[]
): Promise<{ inserted: number; skipped: number }> {
  let inserted = 0;
  let skipped = 0;

  for (const change of changes) {
    // Check if already exists by source_id
    if (change.sourceId) {
      const existing = await db
        .select({ id: seriesFees.id })
        .from(seriesFees)
        .where(eq(seriesFees.sourceId, change.sourceId))
        .limit(1);

      if (existing.length > 0) {
        skipped++;
        continue;
      }
    }

    // Close any existing open fee for this series
    await db
      .update(seriesFees)
      .set({ effectiveTo: change.effectiveFrom })
      .where(
        and(
          eq(seriesFees.seriesTicker, change.seriesTicker),
          isNull(seriesFees.effectiveTo)
        )
      );

    // Insert new fee
    await db.insert(seriesFees).values(change);
    inserted++;
  }

  return { inserted, skipped };
}

/**
 * Get current fee for a series (effective_to IS NULL).
 */
export async function getCurrentFee(
  db: Database,
  seriesTicker: string
): Promise<SeriesFee | null> {
  const rows = await db
    .select()
    .from(seriesFees)
    .where(
      and(
        eq(seriesFees.seriesTicker, seriesTicker),
        isNull(seriesFees.effectiveTo)
      )
    )
    .limit(1);

  return rows[0] ?? null;
}

/**
 * Get fee for a series at a specific point in time.
 */
export async function getFeeAsOf(
  db: Database,
  seriesTicker: string,
  asOf: Date
): Promise<SeriesFee | null> {
  const rows = await db
    .select()
    .from(seriesFees)
    .where(
      and(
        eq(seriesFees.seriesTicker, seriesTicker),
        lte(seriesFees.effectiveFrom, asOf),
        or(isNull(seriesFees.effectiveTo), gte(seriesFees.effectiveTo, asOf))
      )
    )
    .limit(1);

  return rows[0] ?? null;
}

/**
 * List all current fees (effective_to IS NULL).
 */
export async function listCurrentFees(
  db: Database,
  options: { limit?: number } = {}
): Promise<SeriesFee[]> {
  const limit = options.limit ?? 100;

  return await db
    .select()
    .from(seriesFees)
    .where(isNull(seriesFees.effectiveTo))
    .orderBy(desc(seriesFees.effectiveFrom))
    .limit(limit);
}

/**
 * Get fee statistics.
 */
export async function getFeeStats(
  db: Database
): Promise<{
  total: number;
  active: number;
  byType: Record<string, number>;
}> {
  // Total count
  const totalResult = await db
    .select({ count: sql<number>`count(*)::int` })
    .from(seriesFees);
  const total = totalResult[0]?.count ?? 0;

  // Active count
  const activeResult = await db
    .select({ count: sql<number>`count(*)::int` })
    .from(seriesFees)
    .where(isNull(seriesFees.effectiveTo));
  const active = activeResult[0]?.count ?? 0;

  // By type
  const typeRows = await db
    .select({
      feeType: seriesFees.feeType,
      count: sql<number>`count(*)::int`,
    })
    .from(seriesFees)
    .where(isNull(seriesFees.effectiveTo))
    .groupBy(seriesFees.feeType);

  const byType: Record<string, number> = {};
  for (const row of typeRows) {
    byType[row.feeType] = row.count;
  }

  return { total, active, byType };
}
```

**Step 2: Verify fees module compiles**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/lib/db/fees.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/lib/db/fees.ts
git commit -m "refactor(db): migrate fees module to Drizzle"
```

---

## Task 5: Migrate Events Module

**Files:**
- Modify: `ssmd-agent/src/lib/db/events.ts`

**Step 1: Rewrite events.ts with Drizzle queries**

```typescript
/**
 * Event database operations using Drizzle ORM
 */
import { eq, isNull, desc, sql, inArray, notInArray, count } from "drizzle-orm";
import type { Database } from "./client.ts";
import { events, markets, type Event, type NewEvent } from "./schema.ts";

const BATCH_SIZE = 500;

export interface BulkResult {
  batches: number;
  total: number;
}

/**
 * Bulk upsert events with 500-row batches for performance.
 */
export async function bulkUpsertEvents(
  db: Database,
  eventList: NewEvent[]
): Promise<BulkResult> {
  if (eventList.length === 0) {
    return { batches: 0, total: 0 };
  }

  let batches = 0;

  for (let i = 0; i < eventList.length; i += BATCH_SIZE) {
    const batch = eventList.slice(i, i + BATCH_SIZE);

    await db
      .insert(events)
      .values(batch)
      .onConflictDoUpdate({
        target: events.eventTicker,
        set: {
          title: sql`excluded.title`,
          category: sql`excluded.category`,
          seriesTicker: sql`excluded.series_ticker`,
          strikeDate: sql`excluded.strike_date`,
          mutuallyExclusive: sql`excluded.mutually_exclusive`,
          status: sql`excluded.status`,
          updatedAt: sql`NOW()`,
          deletedAt: sql`NULL`,
        },
      });

    batches++;
    console.log(`  [DB] events batch ${batches}: ${batch.length} upserted`);
  }

  return { batches, total: eventList.length };
}

/**
 * Get set of existing event tickers for FK validation.
 */
export async function getExistingEventTickers(
  db: Database,
  eventTickers: string[]
): Promise<Set<string>> {
  if (eventTickers.length === 0) {
    return new Set();
  }

  const rows = await db
    .select({ eventTicker: events.eventTicker })
    .from(events)
    .where(inArray(events.eventTicker, eventTickers));

  return new Set(rows.map((r) => r.eventTicker));
}

/**
 * Soft delete events that are no longer in the API response.
 */
export async function softDeleteMissingEvents(
  db: Database,
  currentTickers: string[]
): Promise<number> {
  if (currentTickers.length === 0) {
    return 0;
  }

  const result = await db
    .update(events)
    .set({ deletedAt: sql`NOW()` })
    .where(notInArray(events.eventTicker, currentTickers));

  return result.rowCount ?? 0;
}

/**
 * List events with optional filters.
 */
export async function listEvents(
  db: Database,
  options: {
    category?: string;
    status?: string;
    series?: string;
    limit?: number;
  } = {}
): Promise<Event[]> {
  const limit = options.limit ?? 100;

  let query = db
    .select()
    .from(events)
    .where(isNull(events.deletedAt))
    .orderBy(desc(events.updatedAt))
    .limit(limit)
    .$dynamic();

  if (options.category) {
    query = query.where(eq(events.category, options.category));
  }
  if (options.status) {
    query = query.where(eq(events.status, options.status));
  }
  if (options.series) {
    query = query.where(eq(events.seriesTicker, options.series));
  }

  return await query;
}

/**
 * Get a single event by ticker with its market count.
 */
export async function getEvent(
  db: Database,
  eventTicker: string
): Promise<(Event & { marketCount: number }) | null> {
  const rows = await db
    .select({
      event: events,
      marketCount: sql<number>`count(${markets.ticker})::int`,
    })
    .from(events)
    .leftJoin(
      markets,
      sql`${markets.eventTicker} = ${events.eventTicker} AND ${markets.deletedAt} IS NULL`
    )
    .where(eq(events.eventTicker, eventTicker))
    .groupBy(events.eventTicker);

  if (rows.length === 0) {
    return null;
  }

  const row = rows[0];
  return {
    ...row.event,
    marketCount: row.marketCount,
  };
}

/**
 * Get event statistics.
 */
export async function getEventStats(
  db: Database
): Promise<{
  total: number;
  byStatus: Record<string, number>;
  byCategory: Record<string, number>;
}> {
  // By status
  const statusRows = await db
    .select({
      status: events.status,
      count: sql<number>`count(*)::int`,
    })
    .from(events)
    .where(isNull(events.deletedAt))
    .groupBy(events.status);

  const byStatus: Record<string, number> = {};
  let total = 0;
  for (const row of statusRows) {
    byStatus[row.status] = row.count;
    total += row.count;
  }

  // By category (top 10)
  const categoryRows = await db
    .select({
      category: events.category,
      count: sql<number>`count(*)::int`,
    })
    .from(events)
    .where(isNull(events.deletedAt))
    .groupBy(events.category)
    .orderBy(desc(sql`count(*)`))
    .limit(10);

  const byCategory: Record<string, number> = {};
  for (const row of categoryRows) {
    byCategory[row.category] = row.count;
  }

  return { total, byStatus, byCategory };
}
```

**Step 2: Verify events module compiles**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/lib/db/events.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/lib/db/events.ts
git commit -m "refactor(db): migrate events module to Drizzle"
```

---

## Task 6: Migrate Markets Module

**Files:**
- Modify: `ssmd-agent/src/lib/db/markets.ts`

**Step 1: Rewrite markets.ts with Drizzle queries**

```typescript
/**
 * Market database operations using Drizzle ORM
 */
import { eq, isNull, desc, sql, inArray, notInArray } from "drizzle-orm";
import type { Database } from "./client.ts";
import { markets, type Market, type NewMarket } from "./schema.ts";

const BATCH_SIZE = 500;

export interface BulkResult {
  batches: number;
  total: number;
}

/**
 * Bulk upsert markets with 500-row batches for performance.
 */
export async function bulkUpsertMarkets(
  db: Database,
  marketList: NewMarket[]
): Promise<BulkResult> {
  if (marketList.length === 0) {
    return { batches: 0, total: 0 };
  }

  let batches = 0;

  for (let i = 0; i < marketList.length; i += BATCH_SIZE) {
    const batch = marketList.slice(i, i + BATCH_SIZE);

    await db
      .insert(markets)
      .values(batch)
      .onConflictDoUpdate({
        target: markets.ticker,
        set: {
          eventTicker: sql`excluded.event_ticker`,
          title: sql`excluded.title`,
          status: sql`excluded.status`,
          closeTime: sql`excluded.close_time`,
          yesBid: sql`excluded.yes_bid`,
          yesAsk: sql`excluded.yes_ask`,
          noBid: sql`excluded.no_bid`,
          noAsk: sql`excluded.no_ask`,
          lastPrice: sql`excluded.last_price`,
          volume: sql`excluded.volume`,
          volume24h: sql`excluded.volume_24h`,
          openInterest: sql`excluded.open_interest`,
          updatedAt: sql`NOW()`,
          deletedAt: sql`NULL`,
        },
      });

    batches++;
    console.log(`  [DB] markets batch ${batches}: ${batch.length} upserted`);
  }

  return { batches, total: marketList.length };
}

/**
 * Soft delete markets that are no longer in the API response.
 */
export async function softDeleteMissingMarkets(
  db: Database,
  currentTickers: string[]
): Promise<number> {
  if (currentTickers.length === 0) {
    return 0;
  }

  const result = await db
    .update(markets)
    .set({ deletedAt: sql`NOW()` })
    .where(notInArray(markets.ticker, currentTickers));

  return result.rowCount ?? 0;
}

/**
 * List markets with optional filters.
 */
export async function listMarkets(
  db: Database,
  options: {
    eventTicker?: string;
    status?: string;
    limit?: number;
  } = {}
): Promise<Market[]> {
  const limit = options.limit ?? 100;

  let query = db
    .select()
    .from(markets)
    .where(isNull(markets.deletedAt))
    .orderBy(desc(markets.updatedAt))
    .limit(limit)
    .$dynamic();

  if (options.eventTicker) {
    query = query.where(eq(markets.eventTicker, options.eventTicker));
  }
  if (options.status) {
    query = query.where(eq(markets.status, options.status));
  }

  return await query;
}

/**
 * Get a single market by ticker.
 */
export async function getMarket(
  db: Database,
  ticker: string
): Promise<Market | null> {
  const rows = await db
    .select()
    .from(markets)
    .where(eq(markets.ticker, ticker))
    .limit(1);

  return rows[0] ?? null;
}

/**
 * Get market statistics.
 */
export async function getMarketStats(
  db: Database
): Promise<{
  total: number;
  byStatus: Record<string, number>;
}> {
  const statusRows = await db
    .select({
      status: markets.status,
      count: sql<number>`count(*)::int`,
    })
    .from(markets)
    .where(isNull(markets.deletedAt))
    .groupBy(markets.status);

  const byStatus: Record<string, number> = {};
  let total = 0;
  for (const row of statusRows) {
    byStatus[row.status] = row.count;
    total += row.count;
  }

  return { total, byStatus };
}
```

**Step 2: Verify markets module compiles**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/lib/db/markets.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/lib/db/markets.ts
git commit -m "refactor(db): migrate markets module to Drizzle"
```

---

## Task 7: Update Module Exports

**Files:**
- Modify: `ssmd-agent/src/lib/db/mod.ts`

**Step 1: Update exports to include schema types**

```typescript
/**
 * Database module exports
 */
export { getDb, getRawSql, closeDb, type Database } from "./client.ts";

// Schema and types
export {
  events,
  markets,
  seriesFees,
  feeTypeEnum,
  type Event,
  type NewEvent,
  type Market,
  type NewMarket,
  type SeriesFee,
  type NewSeriesFee,
} from "./schema.ts";

// Event operations
export {
  bulkUpsertEvents,
  getExistingEventTickers,
  softDeleteMissingEvents,
  listEvents,
  getEvent,
  getEventStats,
  type BulkResult,
} from "./events.ts";

// Market operations
export {
  bulkUpsertMarkets,
  softDeleteMissingMarkets,
  listMarkets,
  getMarket,
  getMarketStats,
} from "./markets.ts";

// Fee operations
export {
  upsertFeeChanges,
  getCurrentFee,
  getFeeAsOf,
  listCurrentFees,
  getFeeStats,
} from "./fees.ts";
```

**Step 2: Verify module compiles**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/lib/db/mod.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/lib/db/mod.ts
git commit -m "refactor(db): update module exports for Drizzle"
```

---

## Task 8: Update Callers - Secmaster Sync

**Files:**
- Modify: `ssmd-agent/src/cli/commands/secmaster.ts`

**Step 1: Update secmaster sync to use new db interface**

The sync command needs to pass `db` instead of `sql` to the bulk upsert functions. Find and update all calls from:
```typescript
const sql = getDb();
await bulkUpsertEvents(sql, events);
```
To:
```typescript
const db = getDb();
await bulkUpsertEvents(db, events);
```

Also update field name references:
- `by_status` → `byStatus`
- `by_category` → `byCategory`
- `market_count` → `marketCount`

**Step 2: Verify secmaster commands compile**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/cli/commands/secmaster.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/cli/commands/secmaster.ts
git commit -m "refactor(cli): update secmaster to use Drizzle db interface"
```

---

## Task 9: Update Callers - Fees CLI

**Files:**
- Modify: `ssmd-agent/src/cli/commands/fees.ts`

**Step 1: Update fees CLI to use new db interface**

Same pattern as secmaster - update `getDb()` calls and field names.

**Step 2: Verify fees commands compile**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/cli/commands/fees.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/cli/commands/fees.ts
git commit -m "refactor(cli): update fees to use Drizzle db interface"
```

---

## Task 10: Update Server Routes

**Files:**
- Modify: `ssmd-agent/src/server/routes.ts`

**Step 1: Update API routes to use new db interface**

Update all route handlers to use `getDb()` returning Drizzle client and camelCase field names.

**Step 2: Verify server compiles**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/server/mod.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/server/routes.ts
git commit -m "refactor(server): update routes to use Drizzle db interface"
```

---

## Task 11: Update Agent Tools

**Files:**
- Modify: `ssmd-agent/src/agent/tools.ts`

**Step 1: Update any direct db calls in agent tools**

If agent tools use direct db queries, update them to use Drizzle.

**Step 2: Verify agent compiles**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/agent/mod.ts
```
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/agent/tools.ts
git commit -m "refactor(agent): update tools to use Drizzle db interface"
```

---

## Task 12: Full Build Verification

**Step 1: Run full type check**

Run:
```bash
cd /workspaces/ssmd/ssmd-agent
deno check src/cli/mod.ts src/server/mod.ts src/agent/mod.ts
```
Expected: No errors

**Step 2: Run tests**

Run:
```bash
cd /workspaces/ssmd
make agent-test
```
Expected: All tests pass

**Step 3: Commit any fixes**

```bash
git add -A
git commit -m "fix: resolve any remaining type errors"
```

---

## Task 13: Integration Test - CLI Commands

**Step 1: Test secmaster commands**

Run (requires DATABASE_URL and SSMD_API_URL):
```bash
cd /workspaces/ssmd/ssmd-agent
deno task cli secmaster stats
deno task cli secmaster events --limit 3
deno task cli secmaster markets --limit 3
```
Expected: Commands return data without errors

**Step 2: Test fees commands**

Run:
```bash
deno task cli fees stats
deno task cli fees list --limit 3
```
Expected: Commands return data without errors

**Step 3: Document results**

If any issues, fix and commit.

---

## Task 14: Cleanup and Final Commit

**Step 1: Remove old type definitions**

Delete any now-unused `EventRow`, `MarketRow` interfaces that were replaced by Drizzle inferred types.

**Step 2: Update imports**

Ensure all files import from `./schema.ts` for types, not old locations.

**Step 3: Final commit**

```bash
git add -A
git commit -m "refactor(db): complete Drizzle ORM migration"
```

---

## Task 15: Merge to Main

**Step 1: Push feature branch**

```bash
cd /workspaces/ssmd
git push origin feature/drizzle-orm
```

**Step 2: Create PR or merge directly**

```bash
git checkout main
git merge feature/drizzle-orm --no-edit
git push origin main
```

**Step 3: Delete feature branch**

```bash
git branch -d feature/drizzle-orm
```

---

## Verification Summary

After completion, verify:
- [ ] `deno check` passes on all modules
- [ ] `make agent-test` passes
- [ ] `ssmd secmaster stats` works
- [ ] `ssmd fees list` works
- [ ] No raw `postgres` template strings remain in db/*.ts
