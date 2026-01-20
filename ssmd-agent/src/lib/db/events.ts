/**
 * Event database operations with upsert support (Drizzle ORM)
 */
import { eq, isNull, desc, sql, inArray, notInArray, count } from "drizzle-orm";
import { type Database, getRawSql } from "./client.ts";
import { events, markets, type Event, type NewEvent } from "./schema.ts";
import type { Event as ApiEvent } from "../types/event.ts";

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

// PostgreSQL has a 65534 parameter limit. Events have ~7 fields, so max safe batch is ~5000.
const EVENTS_BATCH_SIZE = 5000;

/**
 * Upsert a batch of events. Caller handles batching (e.g., API pagination).
 * Uses ON CONFLICT DO UPDATE to handle existing records.
 * Automatically chunks large batches to avoid PostgreSQL's 65534 parameter limit.
 */
export async function upsertEvents(
  db: Database,
  eventList: ApiEvent[]
): Promise<number> {
  if (eventList.length === 0) {
    return 0;
  }

  // Deduplicate by event_ticker (keep last occurrence)
  const seen = new Map<string, ApiEvent>();
  for (const e of eventList) {
    seen.set(e.event_ticker, e);
  }
  const dedupedList = Array.from(seen.values());

  const drizzleEvents = dedupedList.map(toNewEvent);

  // Chunk to avoid PostgreSQL parameter limit (65534)
  for (let i = 0; i < drizzleEvents.length; i += EVENTS_BATCH_SIZE) {
    const chunk = drizzleEvents.slice(i, i + EVENTS_BATCH_SIZE);
    await db
      .insert(events)
      .values(chunk)
      .onConflictDoUpdate({
        target: events.eventTicker,
        set: {
          title: sql`excluded.title`,
          category: sql`excluded.category`,
          seriesTicker: sql`excluded.series_ticker`,
          strikeDate: sql`excluded.strike_date`,
          mutuallyExclusive: sql`excluded.mutually_exclusive`,
          status: sql`excluded.status`,
          // updated_at is handled by trigger (only updates when data changes)
          deletedAt: sql`NULL`,
        },
      });
  }

  return dedupedList.length;
}

/**
 * @deprecated Use upsertEvents instead. This wrapper exists for backward compatibility.
 */
export async function bulkUpsertEvents(
  db: Database,
  eventList: ApiEvent[]
): Promise<{ batches: number; total: number }> {
  const total = await upsertEvents(db, eventList);
  return { batches: 1, total };
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
 * Uses temp table approach to avoid PostgreSQL's 65534 parameter limit.
 */
export async function softDeleteMissingEvents(
  db: Database,
  currentTickers: string[]
): Promise<number> {
  if (currentTickers.length === 0) {
    return 0;
  }

  // Use raw SQL for temp table operations
  const rawSql = getRawSql();

  // Create temp table
  await rawSql`CREATE TEMP TABLE IF NOT EXISTS temp_current_events (event_ticker TEXT PRIMARY KEY)`;
  await rawSql`TRUNCATE temp_current_events`;

  // Insert tickers in batches (10000 per batch to stay well under parameter limit)
  const BATCH_SIZE = 10000;
  for (let i = 0; i < currentTickers.length; i += BATCH_SIZE) {
    const batch = currentTickers.slice(i, i + BATCH_SIZE);
    await rawSql`INSERT INTO temp_current_events (event_ticker) VALUES ${rawSql(batch.map(t => [t]))} ON CONFLICT DO NOTHING`;
  }

  // Soft delete events not in temp table
  const result = await rawSql`
    UPDATE events
    SET deleted_at = NOW()
    WHERE deleted_at IS NULL
      AND event_ticker NOT IN (SELECT event_ticker FROM temp_current_events)
    RETURNING event_ticker
  `;

  return result.length;
}

/**
 * Event row from database (alias for schema Event type)
 */
export type EventRow = Event;

/**
 * List events with optional filters.
 * @param options.asOf - Point-in-time filter (ISO timestamp). Returns events that existed
 *                       at this time. Defaults to now.
 */
export async function listEvents(
  db: Database,
  options: {
    category?: string;
    status?: string;
    series?: string;
    asOf?: string;
    limit?: number;
  } = {}
): Promise<(EventRow & { marketCount: number })[]> {
  const limit = options.limit ?? 100;
  const asOf = options.asOf ?? new Date().toISOString();

  // Build conditions array with point-in-time filtering
  const conditions: ReturnType<typeof sql>[] = [
    // Event existed at this time
    sql`${events.createdAt} <= ${asOf}`,
    // Event wasn't soft-deleted yet
    sql`(${events.deletedAt} IS NULL OR ${events.deletedAt} > ${asOf})`,
  ];

  if (options.category) {
    conditions.push(eq(events.category, options.category));
  }
  if (options.status) {
    conditions.push(eq(events.status, options.status));
  }
  if (options.series) {
    // Case-insensitive match (Kalshi tickers are uppercase but allow lowercase input)
    conditions.push(sql`LOWER(${events.seriesTicker}) = LOWER(${options.series})`);
  }

  return await db
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
    .where(sql.join(conditions, sql` AND `))
    .groupBy(events.eventTicker)
    .orderBy(desc(events.updatedAt))
    .limit(limit);
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
