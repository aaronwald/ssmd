/**
 * Series database operations
 */
import { eq, and, sql, arrayContains } from "drizzle-orm";
import { getDb } from "./client.ts";
import { series, type Series, type NewSeries } from "./schema.ts";

/**
 * Upsert series records (insert or update on conflict)
 */
export async function upsertSeries(records: NewSeries[]): Promise<{ inserted: number; updated: number }> {
  if (records.length === 0) {
    return { inserted: 0, updated: 0 };
  }

  const db = getDb();
  let inserted = 0;
  let updated = 0;

  for (const record of records) {
    const result = await db
      .insert(series)
      .values(record)
      .onConflictDoUpdate({
        target: series.ticker,
        set: {
          title: record.title,
          category: record.category,
          tags: record.tags,
          isGame: record.isGame,
          active: record.active ?? true,
          updatedAt: new Date(),
        },
      })
      .returning({ ticker: series.ticker, createdAt: series.createdAt, updatedAt: series.updatedAt });

    // If createdAt equals updatedAt (within 1 second), it's a new insert
    if (result[0]) {
      const diff = Math.abs(
        new Date(result[0].updatedAt).getTime() - new Date(result[0].createdAt).getTime()
      );
      if (diff < 1000) {
        inserted++;
      } else {
        updated++;
      }
    }
  }

  return { inserted, updated };
}

/**
 * Get series by tags (uses GIN index)
 * @param tags - Array of tags to match (ANY)
 * @param gamesOnly - If true, only return series where is_game = true
 */
export async function getSeriesByTags(
  tags: string[],
  gamesOnly = false
): Promise<Series[]> {
  const db = getDb();

  // Use raw SQL for array overlap (&&) which checks if any tag matches
  // Format tags as PostgreSQL array literal: '{tag1,tag2}'
  const tagsLiteral = `{${tags.join(",")}}`;
  const tagCondition = sql`${series.tags} && ${tagsLiteral}::text[]`;

  if (gamesOnly) {
    return db
      .select()
      .from(series)
      .where(and(tagCondition, eq(series.isGame, true), eq(series.active, true)));
  }

  return db
    .select()
    .from(series)
    .where(and(tagCondition, eq(series.active, true)));
}

/**
 * Get series by category
 * @param category - Category to filter by
 * @param gamesOnly - If true, only return series where is_game = true
 */
export async function getSeriesByCategory(
  category: string,
  gamesOnly = false
): Promise<Series[]> {
  const db = getDb();

  if (gamesOnly) {
    return db
      .select()
      .from(series)
      .where(
        and(
          eq(series.category, category),
          eq(series.isGame, true),
          eq(series.active, true)
        )
      );
  }

  return db
    .select()
    .from(series)
    .where(and(eq(series.category, category), eq(series.active, true)));
}

/**
 * Get all active series
 */
export async function getAllActiveSeries(): Promise<Series[]> {
  const db = getDb();
  return db.select().from(series).where(eq(series.active, true));
}

/**
 * Get series stats (count by category)
 */
export async function getSeriesStats(): Promise<
  Array<{ category: string; total: number; games: number }>
> {
  const db = getDb();

  const result = await db
    .select({
      category: series.category,
      total: sql<number>`count(*)::int`,
      games: sql<number>`sum(case when ${series.isGame} then 1 else 0 end)::int`,
    })
    .from(series)
    .where(eq(series.active, true))
    .groupBy(series.category);

  return result;
}

/**
 * Get a single series by ticker
 */
export async function getSeries(ticker: string): Promise<Series | null> {
  const db = getDb();
  const result = await db
    .select()
    .from(series)
    .where(eq(series.ticker, ticker))
    .limit(1);
  return result[0] || null;
}

/**
 * List all series with optional filters
 */
export async function listSeries(options?: {
  category?: string;
  tag?: string;
  gamesOnly?: boolean;
  limit?: number;
  offset?: number;
}): Promise<Series[]> {
  const db = getDb();

  const conditions = [eq(series.active, true)];

  if (options?.category) {
    conditions.push(eq(series.category, options.category));
  }

  if (options?.tag) {
    conditions.push(sql`${series.tags} && ARRAY[${options.tag}]::text[]`);
  }

  if (options?.gamesOnly) {
    conditions.push(eq(series.isGame, true));
  }

  let query = db
    .select()
    .from(series)
    .where(and(...conditions))
    .orderBy(series.category, series.ticker);

  if (options?.limit) {
    query = query.limit(options.limit) as typeof query;
  }

  if (options?.offset) {
    query = query.offset(options.offset) as typeof query;
  }

  return query;
}
