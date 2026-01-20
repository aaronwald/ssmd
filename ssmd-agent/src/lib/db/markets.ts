/**
 * Market database operations with upsert support (Drizzle ORM)
 */
import { eq, isNull, desc, sql, notInArray, count } from "drizzle-orm";
import { type Database, getRawSql } from "./client.ts";
import { markets, events, type Market, type NewMarket } from "./schema.ts";
import { getExistingEventTickers } from "./events.ts";
import type { Market as ApiMarket } from "../types/market.ts";

export interface UpsertResult {
  total: number;
  skipped: number;
}

/**
 * Convert API market type (snake_case) to Drizzle schema type (camelCase)
 */
function toNewMarket(m: ApiMarket): NewMarket {
  return {
    ticker: m.ticker,
    eventTicker: m.event_ticker,
    title: m.title,
    status: m.status,
    closeTime: m.close_time ? new Date(m.close_time) : null,
    yesBid: m.yes_bid ?? null,
    yesAsk: m.yes_ask ?? null,
    noBid: m.no_bid ?? null,
    noAsk: m.no_ask ?? null,
    lastPrice: m.last_price ?? null,
    volume: m.volume ?? 0,
    volume24h: m.volume_24h ?? 0,
    openInterest: m.open_interest ?? 0,
  };
}

// PostgreSQL has a 65534 parameter limit. Markets have ~14 fields, so max safe batch is ~3000.
const MARKETS_BATCH_SIZE = 3000;

/**
 * Upsert a batch of markets. Caller handles batching (e.g., API pagination).
 * Fails if any markets reference missing parent events (FK constraint).
 * Automatically chunks large batches to avoid PostgreSQL's 65534 parameter limit.
 */
export async function upsertMarkets(
  db: Database,
  marketList: ApiMarket[]
): Promise<UpsertResult> {
  if (marketList.length === 0) {
    return { total: 0, skipped: 0 };
  }

  // Collect unique event tickers
  const eventTickers = [...new Set(marketList.map((m) => m.event_ticker))];

  // Check for missing parent events (FK constraint)
  const existingEvents = await getExistingEventTickers(db, eventTickers);
  const missingEvents = eventTickers.filter((t) => !existingEvents.has(t));

  if (missingEvents.length > 0) {
    const sample = missingEvents.slice(0, 5).join(", ");
    const more = missingEvents.length > 5 ? ` (and ${missingEvents.length - 5} more)` : "";
    throw new Error(
      `FK constraint: ${missingEvents.length} parent events missing: ${sample}${more}. ` +
      `Sync events before markets.`
    );
  }

  const drizzleMarkets = marketList.map(toNewMarket);

  // Chunk to avoid PostgreSQL parameter limit (65534)
  for (let i = 0; i < drizzleMarkets.length; i += MARKETS_BATCH_SIZE) {
    const chunk = drizzleMarkets.slice(i, i + MARKETS_BATCH_SIZE);
    await db
      .insert(markets)
      .values(chunk)
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
          // updated_at is handled by trigger (only updates when data changes)
          deletedAt: sql`NULL`,
        },
      });
  }

  return { total: marketList.length, skipped: 0 };
}

/**
 * @deprecated Use upsertMarkets instead. This wrapper exists for backward compatibility.
 */
export async function bulkUpsertMarkets(
  db: Database,
  marketList: ApiMarket[]
): Promise<{ batches: number; total: number; skipped: number }> {
  const result = await upsertMarkets(db, marketList);
  return { batches: 1, ...result };
}

/**
 * Soft delete markets that are no longer in the API response.
 * Uses temp table approach to avoid PostgreSQL's 65534 parameter limit.
 */
export async function softDeleteMissingMarkets(
  db: Database,
  currentTickers: string[]
): Promise<number> {
  if (currentTickers.length === 0) {
    return 0;
  }

  // Use raw SQL for temp table operations
  const rawSql = getRawSql();

  // Create temp table
  await rawSql`CREATE TEMP TABLE IF NOT EXISTS temp_current_markets (ticker TEXT PRIMARY KEY)`;
  await rawSql`TRUNCATE temp_current_markets`;

  // Insert tickers in batches (10000 per batch to stay well under parameter limit)
  const BATCH_SIZE = 10000;
  for (let i = 0; i < currentTickers.length; i += BATCH_SIZE) {
    const batch = currentTickers.slice(i, i + BATCH_SIZE);
    await rawSql`INSERT INTO temp_current_markets (ticker) VALUES ${rawSql(batch.map(t => [t]))} ON CONFLICT DO NOTHING`;
  }

  // Soft delete markets not in temp table
  const result = await rawSql`
    UPDATE markets
    SET deleted_at = NOW()
    WHERE deleted_at IS NULL
      AND ticker NOT IN (SELECT ticker FROM temp_current_markets)
    RETURNING ticker
  `;

  return result.length;
}

/**
 * Market row from database (alias for schema Market type)
 */
export type MarketRow = Market;

/**
 * List markets with optional filters.
 * @param options.asOf - Point-in-time filter (ISO timestamp). Returns markets that existed
 *                       and were tradeable at this time. Defaults to now.
 */
export async function listMarkets(
  db: Database,
  options: {
    category?: string;
    status?: string;
    series?: string;
    eventTicker?: string;
    closingBefore?: string;
    closingAfter?: string;
    asOf?: string;
    limit?: number;
  } = {}
): Promise<MarketRow[]> {
  const limit = options.limit ?? 100;
  const asOf = options.asOf ?? new Date().toISOString();

  // Build conditions array with point-in-time filtering
  const conditions: ReturnType<typeof sql>[] = [
    // Market existed at this time
    sql`${markets.createdAt} <= ${asOf}`,
    // Market was still tradeable (hadn't closed yet)
    sql`(${markets.closeTime} > ${asOf} OR ${markets.closeTime} IS NULL)`,
    // Market wasn't soft-deleted yet
    sql`(${markets.deletedAt} IS NULL OR ${markets.deletedAt} > ${asOf})`,
  ];

  if (options.status) {
    conditions.push(eq(markets.status, options.status));
  }
  if (options.eventTicker) {
    conditions.push(eq(markets.eventTicker, options.eventTicker));
  }
  if (options.closingBefore) {
    conditions.push(sql`${markets.closeTime} < ${options.closingBefore}`);
  }
  if (options.closingAfter) {
    conditions.push(sql`${markets.closeTime} > ${options.closingAfter}`);
  }

  // If filtering by category or series, need to join events
  if (options.category || options.series) {
    const eventConditions: ReturnType<typeof sql>[] = [];
    if (options.category) {
      eventConditions.push(eq(events.category, options.category));
    }
    if (options.series) {
      // Case-insensitive match (Kalshi tickers are uppercase but allow lowercase input)
      eventConditions.push(sql`LOWER(${events.seriesTicker}) = LOWER(${options.series})`);
    }

    const rows = await db
      .select({
        ticker: markets.ticker,
        eventTicker: markets.eventTicker,
        title: markets.title,
        status: markets.status,
        closeTime: markets.closeTime,
        yesBid: markets.yesBid,
        yesAsk: markets.yesAsk,
        noBid: markets.noBid,
        noAsk: markets.noAsk,
        lastPrice: markets.lastPrice,
        volume: markets.volume,
        volume24h: markets.volume24h,
        openInterest: markets.openInterest,
        createdAt: markets.createdAt,
        updatedAt: markets.updatedAt,
        deletedAt: markets.deletedAt,
      })
      .from(markets)
      .innerJoin(events, eq(markets.eventTicker, events.eventTicker))
      .where(sql.join([...conditions, ...eventConditions], sql` AND `))
      .orderBy(desc(markets.updatedAt))
      .limit(limit);

    return rows;
  }

  // Simple query without join
  const rows = await db
    .select()
    .from(markets)
    .where(sql.join(conditions, sql` AND `))
    .orderBy(desc(markets.updatedAt))
    .limit(limit);

  return rows;
}

/**
 * Get a single market by ticker.
 */
export async function getMarket(
  db: Database,
  ticker: string
): Promise<MarketRow | null> {
  const rows = await db
    .select()
    .from(markets)
    .where(
      sql`${eq(markets.ticker, ticker)} AND ${isNull(markets.deletedAt)}`
    );

  if (rows.length === 0) {
    return null;
  }

  return rows[0];
}

/**
 * Get market statistics by status.
 */
export async function getMarketStats(
  db: Database
): Promise<{ total: number; by_status: Record<string, number> }> {
  const statusRows = await db
    .select({
      status: markets.status,
      count: count(),
    })
    .from(markets)
    .where(isNull(markets.deletedAt))
    .groupBy(markets.status);

  const by_status: Record<string, number> = {};
  let total = 0;
  for (const row of statusRows) {
    by_status[row.status] = row.count;
    total += row.count;
  }

  return { total, by_status };
}

/**
 * Market activity for a single day
 */
export interface MarketDayActivity {
  date: string;
  added: number;
  closed: number;
  settled: number;
}

/**
 * Get market activity over time (added, closed, and settled per day).
 * @param days Number of days to look back (default 30)
 */
export async function getMarketTimeseries(
  db: Database,
  days = 30
): Promise<MarketDayActivity[]> {
  const startDate = new Date();
  startDate.setDate(startDate.getDate() - days);
  const startDateStr = startDate.toISOString().split("T")[0];

  // Get markets added per day (by created_at)
  const addedRows = await db
    .select({
      date: sql<string>`DATE(${markets.createdAt})`.as("date"),
      count: count(),
    })
    .from(markets)
    .where(sql`${markets.createdAt} >= ${startDateStr}`)
    .groupBy(sql`DATE(${markets.createdAt})`)
    .orderBy(sql`DATE(${markets.createdAt})`);

  // Get markets closed per day (status = 'closed')
  const closedRows = await db
    .select({
      date: sql<string>`DATE(${markets.closeTime})`.as("date"),
      count: count(),
    })
    .from(markets)
    .where(
      sql`${markets.closeTime} >= ${startDateStr} AND ${markets.closeTime} <= NOW() AND ${markets.status} = 'closed'`
    )
    .groupBy(sql`DATE(${markets.closeTime})`)
    .orderBy(sql`DATE(${markets.closeTime})`);

  // Get markets settled per day (status = 'settled')
  const settledRows = await db
    .select({
      date: sql<string>`DATE(${markets.closeTime})`.as("date"),
      count: count(),
    })
    .from(markets)
    .where(
      sql`${markets.closeTime} >= ${startDateStr} AND ${markets.closeTime} <= NOW() AND ${markets.status} = 'settled'`
    )
    .groupBy(sql`DATE(${markets.closeTime})`)
    .orderBy(sql`DATE(${markets.closeTime})`);

  // Build a map of all dates in range
  const dateMap = new Map<string, MarketDayActivity>();
  for (let i = 0; i <= days; i++) {
    const d = new Date();
    d.setDate(d.getDate() - (days - i));
    const dateStr = d.toISOString().split("T")[0];
    dateMap.set(dateStr, { date: dateStr, added: 0, closed: 0, settled: 0 });
  }

  // Fill in added counts
  for (const row of addedRows) {
    const entry = dateMap.get(row.date);
    if (entry) {
      entry.added = row.count;
    }
  }

  // Fill in closed counts
  for (const row of closedRows) {
    const entry = dateMap.get(row.date);
    if (entry) {
      entry.closed = row.count;
    }
  }

  // Fill in settled counts
  for (const row of settledRows) {
    const entry = dateMap.get(row.date);
    if (entry) {
      entry.settled = row.count;
    }
  }

  return Array.from(dateMap.values());
}
