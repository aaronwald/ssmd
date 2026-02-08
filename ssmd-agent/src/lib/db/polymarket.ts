/**
 * Polymarket database operations for conditions and tokens (Drizzle ORM)
 */
import { eq, isNull, desc, sql, count } from "drizzle-orm";
import { type Database, getRawSql } from "./client.ts";
import {
  polymarketConditions,
  polymarketTokens,
  type NewPolymarketCondition,
  type NewPolymarketToken,
  type PolymarketCondition,
  type PolymarketToken,
} from "./schema.ts";

const CONDITIONS_BATCH_SIZE = 500;
const TOKENS_BATCH_SIZE = 500;

/**
 * Upsert a batch of Polymarket conditions.
 */
export async function upsertConditions(
  db: Database,
  conditions: NewPolymarketCondition[],
): Promise<number> {
  if (conditions.length === 0) return 0;

  for (let i = 0; i < conditions.length; i += CONDITIONS_BATCH_SIZE) {
    const chunk = conditions.slice(i, i + CONDITIONS_BATCH_SIZE);
    await db
      .insert(polymarketConditions)
      .values(chunk)
      .onConflictDoUpdate({
        target: polymarketConditions.conditionId,
        set: {
          question: sql`excluded.question`,
          slug: sql`excluded.slug`,
          category: sql`excluded.category`,
          outcomes: sql`excluded.outcomes`,
          status: sql`excluded.status`,
          active: sql`excluded.active`,
          endDate: sql`excluded.end_date`,
          resolutionDate: sql`excluded.resolution_date`,
          winningOutcome: sql`excluded.winning_outcome`,
          volume: sql`excluded.volume`,
          liquidity: sql`excluded.liquidity`,
          deletedAt: sql`NULL`,
        },
      });
  }

  return conditions.length;
}

/**
 * Upsert a batch of Polymarket tokens.
 */
export async function upsertTokens(
  db: Database,
  tokens: NewPolymarketToken[],
): Promise<number> {
  if (tokens.length === 0) return 0;

  for (let i = 0; i < tokens.length; i += TOKENS_BATCH_SIZE) {
    const chunk = tokens.slice(i, i + TOKENS_BATCH_SIZE);
    await db
      .insert(polymarketTokens)
      .values(chunk)
      .onConflictDoUpdate({
        target: polymarketTokens.tokenId,
        set: {
          conditionId: sql`excluded.condition_id`,
          outcome: sql`excluded.outcome`,
          outcomeIndex: sql`excluded.outcome_index`,
          price: sql`excluded.price`,
          bid: sql`excluded.bid`,
          ask: sql`excluded.ask`,
          volume: sql`excluded.volume`,
        },
      });
  }

  return tokens.length;
}

/**
 * Soft delete conditions not in the provided list.
 * Uses temp table approach to avoid PostgreSQL's 65534 parameter limit.
 */
export async function softDeleteMissingConditions(
  currentConditionIds: string[],
): Promise<number> {
  if (currentConditionIds.length === 0) return 0;

  const rawSql = getRawSql();

  await rawSql`CREATE TEMP TABLE IF NOT EXISTS temp_current_conditions (condition_id TEXT PRIMARY KEY)`;
  await rawSql`TRUNCATE temp_current_conditions`;

  const BATCH_SIZE = 10000;
  for (let i = 0; i < currentConditionIds.length; i += BATCH_SIZE) {
    const batch = currentConditionIds.slice(i, i + BATCH_SIZE);
    await rawSql`INSERT INTO temp_current_conditions (condition_id) VALUES ${rawSql(batch.map((t) => [t]))} ON CONFLICT DO NOTHING`;
  }

  const result = await rawSql`
    UPDATE polymarket_conditions
    SET deleted_at = NOW()
    WHERE deleted_at IS NULL
      AND condition_id NOT IN (SELECT condition_id FROM temp_current_conditions)
    RETURNING condition_id
  `;

  return result.length;
}

/**
 * List Polymarket conditions with optional filters.
 */
export async function listConditions(
  db: Database,
  options: {
    category?: string;
    status?: string;
    limit?: number;
  } = {},
): Promise<(PolymarketCondition & { tokenCount: number })[]> {
  const limit = options.limit ?? 100;

  const conditions: ReturnType<typeof sql>[] = [
    isNull(polymarketConditions.deletedAt),
  ];

  if (options.category) {
    conditions.push(eq(polymarketConditions.category, options.category));
  }
  if (options.status) {
    conditions.push(eq(polymarketConditions.status, options.status));
  }

  return await db
    .select({
      conditionId: polymarketConditions.conditionId,
      question: polymarketConditions.question,
      slug: polymarketConditions.slug,
      category: polymarketConditions.category,
      outcomes: polymarketConditions.outcomes,
      status: polymarketConditions.status,
      active: polymarketConditions.active,
      endDate: polymarketConditions.endDate,
      resolutionDate: polymarketConditions.resolutionDate,
      winningOutcome: polymarketConditions.winningOutcome,
      volume: polymarketConditions.volume,
      liquidity: polymarketConditions.liquidity,
      createdAt: polymarketConditions.createdAt,
      updatedAt: polymarketConditions.updatedAt,
      deletedAt: polymarketConditions.deletedAt,
      tokenCount: count(polymarketTokens.tokenId),
    })
    .from(polymarketConditions)
    .leftJoin(
      polymarketTokens,
      eq(polymarketTokens.conditionId, polymarketConditions.conditionId),
    )
    .where(sql.join(conditions, sql` AND `))
    .groupBy(polymarketConditions.conditionId)
    .orderBy(desc(polymarketConditions.updatedAt))
    .limit(limit);
}

/**
 * Get a single Polymarket condition with its tokens.
 */
export async function getCondition(
  db: Database,
  conditionId: string,
): Promise<{ condition: PolymarketCondition; tokens: PolymarketToken[] } | null> {
  const rows = await db
    .select()
    .from(polymarketConditions)
    .where(
      sql`${eq(polymarketConditions.conditionId, conditionId)} AND ${isNull(polymarketConditions.deletedAt)}`,
    );

  if (rows.length === 0) return null;

  const tokens = await db
    .select()
    .from(polymarketTokens)
    .where(eq(polymarketTokens.conditionId, conditionId));

  return { condition: rows[0], tokens };
}

/**
 * List token IDs for conditions matching the given categories and status.
 * JOINs polymarket_tokens → polymarket_conditions filtered by category.
 * Used by connectors to get secmaster-driven subscription lists.
 */
export async function listTokensByCategories(
  db: Database,
  options: {
    categories: string[];
    status?: string;
  },
): Promise<string[]> {
  if (options.categories.length === 0) return [];

  const conditions: ReturnType<typeof sql>[] = [
    isNull(polymarketConditions.deletedAt),
  ];

  // Filter by categories using IN clause
  conditions.push(
    sql`${polymarketConditions.category} IN ${options.categories}`,
  );

  if (options.status) {
    conditions.push(eq(polymarketConditions.status, options.status));
  }

  const rows = await db
    .select({
      tokenId: polymarketTokens.tokenId,
    })
    .from(polymarketTokens)
    .innerJoin(
      polymarketConditions,
      eq(polymarketTokens.conditionId, polymarketConditions.conditionId),
    )
    .where(sql.join(conditions, sql` AND `));

  return rows.map((r) => r.tokenId);
}

/**
 * Get Polymarket condition statistics.
 */
export async function getConditionStats(
  db: Database,
): Promise<{
  total: number;
  by_status: Record<string, number>;
  by_category: Record<string, number>;
}> {
  const statusRows = await db
    .select({
      status: polymarketConditions.status,
      count: count(),
    })
    .from(polymarketConditions)
    .where(isNull(polymarketConditions.deletedAt))
    .groupBy(polymarketConditions.status);

  const categoryRows = await db
    .select({
      category: polymarketConditions.category,
      count: count(),
    })
    .from(polymarketConditions)
    .where(
      sql`${isNull(polymarketConditions.deletedAt)} AND ${polymarketConditions.category} IS NOT NULL`,
    )
    .groupBy(polymarketConditions.category)
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
    if (row.category) {
      by_category[row.category] = row.count;
    }
  }

  return { total, by_status, by_category };
}
