/**
 * Event database operations with bulk upsert support (Drizzle ORM)
 */
import { eq, isNull, desc, sql, inArray, notInArray, count } from "drizzle-orm";
import type { Database } from "./client.ts";
import { events, markets, type Event, type NewEvent } from "./schema.ts";
import type { Event as ApiEvent } from "../types/event.ts";

const BATCH_SIZE = 500;

export interface BulkResult {
  batches: number;
  total: number;
}

/**
 * Convert API event type (snake_case) to Drizzle schema type (camelCase)
 */
function toNewEvent(e: ApiEvent): NewEvent {
  return {
    eventTicker: e.event_ticker,
    title: e.title,
    category: e.category,
    seriesTicker: e.series_ticker ?? undefined,
    strikeDate: e.strike_date ? new Date(e.strike_date) : null,
    mutuallyExclusive: e.mutually_exclusive ?? false,
    status: e.status ?? "active",
  };
}

/**
 * Bulk upsert events with 500-row batches for performance.
 * Matches Go implementation's performance characteristics.
 * Accepts API event type (snake_case) and converts to Drizzle schema type.
 */
export async function bulkUpsertEvents(
  db: Database,
  eventList: ApiEvent[]
): Promise<BulkResult> {
  if (eventList.length === 0) {
    return { batches: 0, total: 0 };
  }

  let batches = 0;

  for (let i = 0; i < eventList.length; i += BATCH_SIZE) {
    const batch = eventList.slice(i, i + BATCH_SIZE);
    // Convert API types to Drizzle schema types
    const drizzleBatch = batch.map(toNewEvent);

    await db
      .insert(events)
      .values(drizzleBatch)
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
 * Used to filter markets before insert to avoid FK violations.
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
    .where(
      sql`${inArray(events.eventTicker, eventTickers)} AND ${isNull(events.deletedAt)}`
    );

  return new Set(rows.map((r) => r.eventTicker));
}

/**
 * Soft delete events that are no longer in the API response.
 */
export async function softDeleteMissingEvents(
  db: Database,
  currentTickers: string[]
): Promise<number> {
  const result = await db
    .update(events)
    .set({ deletedAt: sql`NOW()` })
    .where(
      sql`${notInArray(events.eventTicker, currentTickers)} AND ${isNull(events.deletedAt)}`
    )
    .returning({ eventTicker: events.eventTicker });

  return result.length;
}

/**
 * Event row from database (alias for schema Event type)
 */
export type EventRow = Event;

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
): Promise<EventRow[]> {
  const limit = options.limit ?? 100;

  let query = db
    .select()
    .from(events)
    .where(isNull(events.deletedAt))
    .orderBy(desc(events.updatedAt))
    .limit(limit)
    .$dynamic();

  if (options.category) {
    query = query.where(
      sql`${isNull(events.deletedAt)} AND ${eq(events.category, options.category)}`
    );
  }
  if (options.status) {
    query = query.where(
      sql`${isNull(events.deletedAt)} AND ${eq(events.status, options.status)}`
    );
  }
  if (options.series) {
    query = query.where(
      sql`${isNull(events.deletedAt)} AND ${eq(events.seriesTicker, options.series)}`
    );
  }

  return await query;
}

/**
 * Get a single event by ticker with its market count.
 */
export async function getEvent(
  db: Database,
  eventTicker: string
): Promise<(EventRow & { marketCount: number }) | null> {
  const rows = await db
    .select({
      eventTicker: events.eventTicker,
      title: events.title,
      category: events.category,
      seriesTicker: events.seriesTicker,
      strikeDate: events.strikeDate,
      mutuallyExclusive: events.mutuallyExclusive,
      status: events.status,
      createdAt: events.createdAt,
      updatedAt: events.updatedAt,
      deletedAt: events.deletedAt,
      marketCount: count(markets.ticker),
    })
    .from(events)
    .leftJoin(
      markets,
      sql`${markets.eventTicker} = ${events.eventTicker} AND ${isNull(markets.deletedAt)}`
    )
    .where(
      sql`${eq(events.eventTicker, eventTicker)} AND ${isNull(events.deletedAt)}`
    )
    .groupBy(events.eventTicker);

  if (rows.length === 0) {
    return null;
  }

  return rows[0];
}

/**
 * Get event statistics.
 */
export async function getEventStats(
  db: Database
): Promise<{
  total: number;
  by_status: Record<string, number>;
  by_category: Record<string, number>;
}> {
  const statusRows = await db
    .select({
      status: events.status,
      count: count(),
    })
    .from(events)
    .where(isNull(events.deletedAt))
    .groupBy(events.status);

  const categoryRows = await db
    .select({
      category: events.category,
      count: count(),
    })
    .from(events)
    .where(isNull(events.deletedAt))
    .groupBy(events.category)
    .orderBy(desc(count()))
    .limit(10);

  const by_status: Record<string, number> = {};
  let total = 0;
  for (const row of statusRows) {
    by_status[row.status] = row.count;
    total += row.count;
  }

  const by_category: Record<string, number> = {};
  for (const row of categoryRows) {
    by_category[row.category] = row.count;
  }

  return { total, by_status, by_category };
}
