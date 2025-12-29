/**
 * Event database operations with bulk upsert support
 */
import type postgres from "postgres";
import type { Event } from "../types/event.ts";

const BATCH_SIZE = 500;

export interface BulkResult {
  batches: number;
  total: number;
}

/**
 * Bulk upsert events with 500-row batches for performance.
 * Matches Go implementation's performance characteristics.
 */
export async function bulkUpsertEvents(
  sql: ReturnType<typeof postgres>,
  events: Event[]
): Promise<BulkResult> {
  if (events.length === 0) {
    return { batches: 0, total: 0 };
  }

  let batches = 0;

  for (let i = 0; i < events.length; i += BATCH_SIZE) {
    const batch = events.slice(i, i + BATCH_SIZE);

    await sql`
      INSERT INTO events ${sql(
        batch,
        "event_ticker",
        "title",
        "category",
        "series_ticker",
        "strike_date",
        "mutually_exclusive",
        "status"
      )}
      ON CONFLICT (event_ticker) DO UPDATE SET
        title = EXCLUDED.title,
        category = EXCLUDED.category,
        series_ticker = EXCLUDED.series_ticker,
        strike_date = EXCLUDED.strike_date,
        mutually_exclusive = EXCLUDED.mutually_exclusive,
        status = EXCLUDED.status,
        updated_at = NOW(),
        deleted_at = NULL
    `;

    batches++;
    console.log(`  [DB] events batch ${batches}: ${batch.length} upserted`);
  }

  return { batches, total: events.length };
}

/**
 * Get set of existing event tickers for FK validation.
 * Used to filter markets before insert to avoid FK violations.
 */
export async function getExistingEventTickers(
  sql: ReturnType<typeof postgres>,
  eventTickers: string[]
): Promise<Set<string>> {
  if (eventTickers.length === 0) {
    return new Set();
  }

  const rows = await sql`
    SELECT event_ticker FROM events
    WHERE event_ticker = ANY(${eventTickers})
    AND deleted_at IS NULL
  `;

  return new Set(rows.map((r) => (r as Record<string, string>).event_ticker));
}

/**
 * Soft delete events that are no longer in the API response.
 */
export async function softDeleteMissingEvents(
  sql: ReturnType<typeof postgres>,
  currentTickers: string[]
): Promise<number> {
  const result = await sql`
    UPDATE events
    SET deleted_at = NOW()
    WHERE event_ticker != ALL(${currentTickers})
    AND deleted_at IS NULL
  `;

  return result.count;
}

/**
 * Event row from database
 */
export interface EventRow {
  event_ticker: string;
  title: string;
  category: string;
  series_ticker: string | null;
  strike_date: string | null;
  mutually_exclusive: boolean;
  status: string;
  created_at: Date;
  updated_at: Date;
}

/**
 * List events with optional filters.
 */
export async function listEvents(
  sql: ReturnType<typeof postgres>,
  options: {
    category?: string;
    status?: string;
    series?: string;
    limit?: number;
  } = {}
): Promise<EventRow[]> {
  const limit = options.limit ?? 100;

  const rows = await sql`
    SELECT event_ticker, title, category, series_ticker, strike_date,
           mutually_exclusive, status, created_at, updated_at
    FROM events
    WHERE deleted_at IS NULL
      ${options.category ? sql`AND category = ${options.category}` : sql``}
      ${options.status ? sql`AND status = ${options.status}` : sql``}
      ${options.series ? sql`AND series_ticker = ${options.series}` : sql``}
    ORDER BY updated_at DESC
    LIMIT ${limit}
  `;

  return rows as unknown as EventRow[];
}

/**
 * Get a single event by ticker with its market count.
 */
export async function getEvent(
  sql: ReturnType<typeof postgres>,
  eventTicker: string
): Promise<(EventRow & { market_count: number }) | null> {
  const rows = await sql`
    SELECT e.event_ticker, e.title, e.category, e.series_ticker, e.strike_date,
           e.mutually_exclusive, e.status, e.created_at, e.updated_at,
           COUNT(m.ticker) as market_count
    FROM events e
    LEFT JOIN markets m ON m.event_ticker = e.event_ticker AND m.deleted_at IS NULL
    WHERE e.event_ticker = ${eventTicker}
      AND e.deleted_at IS NULL
    GROUP BY e.event_ticker
  `;

  if (rows.length === 0) {
    return null;
  }

  const row = rows[0] as Record<string, unknown>;
  return {
    ...row,
    market_count: Number(row.market_count),
  } as EventRow & { market_count: number };
}

/**
 * Get event statistics.
 */
export async function getEventStats(
  sql: ReturnType<typeof postgres>
): Promise<{ total: number; by_status: Record<string, number>; by_category: Record<string, number> }> {
  const statusRows = await sql`
    SELECT status, COUNT(*) as count
    FROM events
    WHERE deleted_at IS NULL
    GROUP BY status
  `;

  const categoryRows = await sql`
    SELECT category, COUNT(*) as count
    FROM events
    WHERE deleted_at IS NULL
    GROUP BY category
    ORDER BY count DESC
    LIMIT 10
  `;

  const by_status: Record<string, number> = {};
  let total = 0;
  for (const row of statusRows) {
    const r = row as Record<string, unknown>;
    by_status[r.status as string] = Number(r.count);
    total += Number(r.count);
  }

  const by_category: Record<string, number> = {};
  for (const row of categoryRows) {
    const r = row as Record<string, unknown>;
    by_category[r.category as string] = Number(r.count);
  }

  return { total, by_status, by_category };
}
