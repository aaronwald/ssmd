/**
 * Market database operations with bulk upsert support
 */
import type postgres from "postgres";
import type { Market } from "../types/market.ts";
import { getExistingEventTickers } from "./events.ts";

const BATCH_SIZE = 500;

export interface MarketBulkResult {
  batches: number;
  total: number;
  skipped: number;
}

/**
 * Bulk upsert markets with 500-row batches.
 * Pre-filters by existing events to avoid FK violations.
 */
export async function bulkUpsertMarkets(
  sql: ReturnType<typeof postgres>,
  markets: Market[]
): Promise<MarketBulkResult> {
  if (markets.length === 0) {
    return { batches: 0, total: 0, skipped: 0 };
  }

  // Collect unique event tickers
  const eventTickers = [...new Set(markets.map((m) => m.event_ticker))];

  // Pre-filter by existing events (FK constraint)
  const existingEvents = await getExistingEventTickers(sql, eventTickers);
  console.log(
    `  [DB] found ${existingEvents.size}/${eventTickers.length} parent events`
  );

  // Filter markets to only those with existing parent events
  const validMarkets = markets.filter((m) => existingEvents.has(m.event_ticker));
  const skipped = markets.length - validMarkets.length;

  if (skipped > 0) {
    console.log(`  [DB] skipping ${skipped} markets with missing events`);
  }

  if (validMarkets.length === 0) {
    return { batches: 0, total: 0, skipped };
  }

  let batches = 0;

  for (let i = 0; i < validMarkets.length; i += BATCH_SIZE) {
    const batch = validMarkets.slice(i, i + BATCH_SIZE);

    await sql`
      INSERT INTO markets ${sql(
        batch,
        "ticker",
        "event_ticker",
        "title",
        "status",
        "close_time",
        "yes_bid",
        "yes_ask",
        "no_bid",
        "no_ask",
        "last_price",
        "volume",
        "volume_24h",
        "open_interest"
      )}
      ON CONFLICT (ticker) DO UPDATE SET
        event_ticker = EXCLUDED.event_ticker,
        title = EXCLUDED.title,
        status = EXCLUDED.status,
        close_time = EXCLUDED.close_time,
        yes_bid = EXCLUDED.yes_bid,
        yes_ask = EXCLUDED.yes_ask,
        no_bid = EXCLUDED.no_bid,
        no_ask = EXCLUDED.no_ask,
        last_price = EXCLUDED.last_price,
        volume = EXCLUDED.volume,
        volume_24h = EXCLUDED.volume_24h,
        open_interest = EXCLUDED.open_interest,
        updated_at = NOW(),
        deleted_at = NULL
    `;

    batches++;
    console.log(`  [DB] markets batch ${batches}: ${batch.length} upserted`);
  }

  return { batches, total: validMarkets.length, skipped };
}

/**
 * Soft delete markets that are no longer in the API response.
 */
export async function softDeleteMissingMarkets(
  sql: ReturnType<typeof postgres>,
  currentTickers: string[]
): Promise<number> {
  const result = await sql`
    UPDATE markets
    SET deleted_at = NOW()
    WHERE ticker != ALL(${currentTickers})
    AND deleted_at IS NULL
  `;

  return result.count;
}

/**
 * Market row from database
 */
export interface MarketRow {
  ticker: string;
  event_ticker: string;
  title: string;
  status: string;
  close_time: Date | null;
  yes_bid: number;
  yes_ask: number;
  no_bid: number;
  no_ask: number;
  last_price: number;
  volume: number;
  volume_24h: number;
  open_interest: number;
  created_at: Date;
  updated_at: Date;
}

/**
 * List markets with optional filters.
 */
export async function listMarkets(
  sql: ReturnType<typeof postgres>,
  options: {
    category?: string;
    status?: string;
    series?: string;
    event?: string;
    closing_before?: string;
    closing_after?: string;
    limit?: number;
  } = {}
): Promise<MarketRow[]> {
  const limit = options.limit ?? 100;

  const rows = await sql`
    SELECT m.ticker, m.event_ticker, m.title, m.status, m.close_time,
           m.yes_bid, m.yes_ask, m.no_bid, m.no_ask, m.last_price,
           m.volume, m.volume_24h, m.open_interest, m.created_at, m.updated_at
    FROM markets m
    JOIN events e ON e.event_ticker = m.event_ticker
    WHERE m.deleted_at IS NULL
      ${options.category ? sql`AND e.category = ${options.category}` : sql``}
      ${options.status ? sql`AND m.status = ${options.status}` : sql``}
      ${options.series ? sql`AND e.series_ticker = ${options.series}` : sql``}
      ${options.event ? sql`AND m.event_ticker = ${options.event}` : sql``}
      ${options.closing_before ? sql`AND m.close_time < ${options.closing_before}` : sql``}
      ${options.closing_after ? sql`AND m.close_time > ${options.closing_after}` : sql``}
    ORDER BY m.updated_at DESC
    LIMIT ${limit}
  `;

  return rows as unknown as MarketRow[];
}

/**
 * Get a single market by ticker.
 */
export async function getMarket(
  sql: ReturnType<typeof postgres>,
  ticker: string
): Promise<MarketRow | null> {
  const rows = await sql`
    SELECT ticker, event_ticker, title, status, close_time,
           yes_bid, yes_ask, no_bid, no_ask, last_price,
           volume, volume_24h, open_interest, created_at, updated_at
    FROM markets
    WHERE ticker = ${ticker}
      AND deleted_at IS NULL
  `;

  if (rows.length === 0) {
    return null;
  }

  return rows[0] as unknown as MarketRow;
}

/**
 * Get market statistics.
 */
export async function getMarketStats(
  sql: ReturnType<typeof postgres>
): Promise<{ total: number; by_status: Record<string, number> }> {
  const statusRows = await sql`
    SELECT status, COUNT(*) as count
    FROM markets
    WHERE deleted_at IS NULL
    GROUP BY status
  `;

  const by_status: Record<string, number> = {};
  let total = 0;
  for (const row of statusRows) {
    const r = row as Record<string, unknown>;
    by_status[r.status as string] = Number(r.count);
    total += Number(r.count);
  }

  return { total, by_status };
}
