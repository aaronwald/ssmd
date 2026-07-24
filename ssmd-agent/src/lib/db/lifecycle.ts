/**
 * Market lifecycle event database operations
 */
import { desc, eq, sql, count, and, gte, lt } from "drizzle-orm";
import { type Database } from "./client.ts";
import { marketLifecycleEvents, type NewMarketLifecycleEvent } from "./schema.ts";

/**
 * Insert a lifecycle event into the database.
 */
export async function insertLifecycleEvent(
  db: Database,
  event: NewMarketLifecycleEvent
): Promise<void> {
  await db.insert(marketLifecycleEvents).values(event);
}

/**
 * Insert multiple lifecycle events in a batch.
 */
export async function insertLifecycleEvents(
  db: Database,
  events: NewMarketLifecycleEvent[]
): Promise<number> {
  if (events.length === 0) return 0;

  // Chunk to avoid PostgreSQL parameter limit
  const BATCH_SIZE = 5000;
  let inserted = 0;

  for (let i = 0; i < events.length; i += BATCH_SIZE) {
    const chunk = events.slice(i, i + BATCH_SIZE);
    await db.insert(marketLifecycleEvents).values(chunk);
    inserted += chunk.length;
  }

  return inserted;
}

/**
 * Get lifecycle events for a specific market ticker.
 */
export async function getLifecycleEventsByMarket(
  db: Database,
  marketTicker: string,
  limit = 100
): Promise<typeof marketLifecycleEvents.$inferSelect[]> {
  return db
    .select()
    .from(marketLifecycleEvents)
    .where(eq(marketLifecycleEvents.marketTicker, marketTicker))
    .orderBy(desc(marketLifecycleEvents.receivedAt))
    .limit(limit);
}

/**
 * Get lifecycle events by event type.
 */
export async function getLifecycleEventsByType(
  db: Database,
  eventType: string,
  limit = 100
): Promise<typeof marketLifecycleEvents.$inferSelect[]> {
  return db
    .select()
    .from(marketLifecycleEvents)
    .where(eq(marketLifecycleEvents.eventType, eventType))
    .orderBy(desc(marketLifecycleEvents.receivedAt))
    .limit(limit);
}

/**
 * Get recent lifecycle events.
 */
export async function getRecentLifecycleEvents(
  db: Database,
  limit = 100
): Promise<typeof marketLifecycleEvents.$inferSelect[]> {
  return db
    .select()
    .from(marketLifecycleEvents)
    .orderBy(desc(marketLifecycleEvents.receivedAt))
    .limit(limit);
}

/**
 * Get lifecycle event statistics.
 */
export async function getLifecycleStats(
  db: Database
): Promise<{
  total: number;
  byEventType: { eventType: string; count: number }[];
}> {
  const [totalResult] = await db
    .select({ count: count() })
    .from(marketLifecycleEvents);

  const byEventType = await db
    .select({
      eventType: marketLifecycleEvents.eventType,
      count: count(),
    })
    .from(marketLifecycleEvents)
    .groupBy(marketLifecycleEvents.eventType)
    .orderBy(desc(count()));

  return {
    total: totalResult?.count ?? 0,
    byEventType: byEventType.map(r => ({
      eventType: r.eventType,
      count: Number(r.count),
    })),
  };
}

/** Strict UTC calendar-date guard (YYYY-MM-DD). Rejects malformed or impossible dates. */
function assertIsoDate(date: string): void {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) {
    throw new Error(`invalid date (expected YYYY-MM-DD): ${JSON.stringify(date)}`);
  }
  // Reject values that pass the regex but aren't real dates (e.g. 2026-13-40).
  const asDate = new Date(`${date}T00:00:00Z`);
  if (Number.isNaN(asDate.getTime()) || asDate.toISOString().slice(0, 10) !== date) {
    throw new Error(`invalid calendar date: ${JSON.stringify(date)}`);
  }
}

/**
 * Return the UTC date (YYYY-MM-DD) of the oldest lifecycle event, or null if empty.
 * Used by retention to enumerate the drop window from oldest date forward.
 */
export async function oldestLifecycleDate(db: Database): Promise<string | null> {
  const [row] = await db
    .select({
      day: sql<string | null>`to_char(min(${marketLifecycleEvents.receivedAt}) AT TIME ZONE 'UTC', 'YYYY-MM-DD')`,
    })
    .from(marketLifecycleEvents);
  const day = row?.day ?? null;
  // Empty table -> null. Any non-null value must be a well-formed date; a
  // malformed value means the query/driver misbehaved -> fail loud, don't
  // hand a bad date to the delete path.
  if (day !== null) assertIsoDate(day);
  return day;
}

/**
 * Delete all lifecycle events whose received_at falls on the given UTC date
 * [date 00:00Z, date+1day 00:00Z). Returns the number of rows deleted.
 * Retention calls this ONLY after confirming the date is archived to GCS parquet.
 */
export async function deleteLifecycleEventsForDate(
  db: Database,
  date: string,
): Promise<number> {
  const deleted = await db
    .delete(marketLifecycleEvents)
    .where(utcDayPredicate(date))
    .returning({ id: marketLifecycleEvents.id });
  return deleted.length;
}

/**
 * Count lifecycle events whose received_at falls on the given UTC date.
 * Retention uses this to skip legitimately-empty days (no rows to protect or delete)
 * so a feed-outage day with no archive manifest can't wedge the gate forever.
 */
export async function countLifecycleEventsForDate(
  db: Database,
  date: string,
): Promise<number> {
  const [row] = await db
    .select({ n: count() })
    .from(marketLifecycleEvents)
    .where(utcDayPredicate(date));
  return Number(row?.n ?? 0);
}

/**
 * Half-open UTC-day window predicate `[date 00:00Z, date+24h)` on received_at.
 *
 * Anchors both bounds to UTC midnight with an explicit +00 offset so the window is
 * exactly one UTC day regardless of the DB session TimeZone. (A bare `${date}::date`
 * cast would resolve to midnight in the session zone, drifting the boundary off UTC
 * and — on the delete path — risking removal of an adjacent, unarchived UTC day.)
 * `received_at` stays a raw column reference so idx_mle_received is still used.
 *
 * The upper bound adds `interval '24 hours'` (a FIXED duration), NOT `interval '1 day'`
 * (calendar arithmetic evaluated in the session zone, which is 23h/25h across a DST
 * transition and would over/under-shoot by an hour on DST days). A UTC day is always
 * exactly 24h, so 24 hours after UTC-midnight is exactly the next UTC-midnight.
 */
function utcDayPredicate(date: string) {
  // Reject any malformed date before it reaches SQL (defense-in-depth on the delete path).
  assertIsoDate(date);
  const dayStartUtc = sql`(${date} || ' 00:00:00+00')::timestamptz`;
  return and(
    gte(marketLifecycleEvents.receivedAt, dayStartUtc),
    lt(marketLifecycleEvents.receivedAt, sql`${dayStartUtc} + interval '24 hours'`),
  );
}
