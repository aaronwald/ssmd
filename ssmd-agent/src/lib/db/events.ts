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
