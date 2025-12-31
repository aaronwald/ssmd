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

/**
 * Upsert a batch of markets. Caller handles batching (e.g., API pagination).
 * Pre-filters by existing events to avoid FK violations.
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

  // Pre-filter by existing events (FK constraint)
  const existingEvents = await getExistingEventTickers(db, eventTickers);

  // Filter markets to only those with existing parent events
  const validMarkets = marketList.filter((m) => existingEvents.has(m.event_ticker));
  const skipped = marketList.length - validMarkets.length;

  if (validMarkets.length === 0) {
    return { total: 0, skipped };
  }

  const drizzleMarkets = validMarkets.map(toNewMarket);

  await db
    .insert(markets)
    .values(drizzleMarkets)
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

  return { total: validMarkets.length, skipped };
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
    limit?: number;
  } = {}
): Promise<MarketRow[]> {
  const limit = options.limit ?? 100;

  // Build conditions array
  const conditions: ReturnType<typeof sql>[] = [isNull(markets.deletedAt)];

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
      eventConditions.push(eq(events.seriesTicker, options.series));
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
