/**
 * Polymarket database operations for conditions and tokens (Drizzle ORM)
 */
import { sql } from "drizzle-orm";
import { type Database, getRawSql } from "./client.ts";
import {
  polymarketConditions,
  polymarketTokens,
  type NewPolymarketCondition,
  type NewPolymarketToken,
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
