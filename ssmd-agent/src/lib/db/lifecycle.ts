/**
 * Market lifecycle event database operations
 */
import { desc, eq, sql, count, and, gte, lte } from "drizzle-orm";
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
