/**
 * Pairs database operations for Kraken spot + perpetual upserts (Drizzle ORM)
 */
import { sql } from "drizzle-orm";
import { type Database, getRawSql } from "./client.ts";
import { pairs, type NewPair } from "./schema.ts";

// PostgreSQL has a 65534 parameter limit. Pairs have many fields, so keep batches conservative.
const PAIRS_BATCH_SIZE = 500;

/**
 * Upsert a batch of spot pairs into the pairs table.
 * Uses ON CONFLICT DO UPDATE on pair_id.
 */
export async function upsertSpotPairs(
  db: Database,
  pairList: NewPair[],
): Promise<number> {
  if (pairList.length === 0) return 0;

  for (let i = 0; i < pairList.length; i += PAIRS_BATCH_SIZE) {
    const chunk = pairList.slice(i, i + PAIRS_BATCH_SIZE);
    await db
      .insert(pairs)
      .values(chunk)
      .onConflictDoUpdate({
        target: pairs.pairId,
        set: {
          exchange: sql`excluded.exchange`,
          base: sql`excluded.base`,
          quote: sql`excluded.quote`,
          wsName: sql`excluded.ws_name`,
          status: sql`excluded.status`,
          lotDecimals: sql`excluded.lot_decimals`,
          pairDecimals: sql`excluded.pair_decimals`,
          marketType: sql`excluded.market_type`,
          altname: sql`excluded.altname`,
          tickSize: sql`excluded.tick_size`,
          orderMin: sql`excluded.order_min`,
          costMin: sql`excluded.cost_min`,
          feeSchedule: sql`excluded.fee_schedule`,
          deletedAt: sql`NULL`,
        },
      });
  }

  return pairList.length;
}

/**
 * Upsert a batch of perpetual pairs into the pairs table.
 * Merges instrument metadata with ticker data.
 */
export async function upsertPerpPairs(
  db: Database,
  pairList: NewPair[],
): Promise<number> {
  if (pairList.length === 0) return 0;

  for (let i = 0; i < pairList.length; i += PAIRS_BATCH_SIZE) {
    const chunk = pairList.slice(i, i + PAIRS_BATCH_SIZE);
    await db
      .insert(pairs)
      .values(chunk)
      .onConflictDoUpdate({
        target: pairs.pairId,
        set: {
          exchange: sql`excluded.exchange`,
          base: sql`excluded.base`,
          quote: sql`excluded.quote`,
          wsName: sql`excluded.ws_name`,
          status: sql`excluded.status`,
          marketType: sql`excluded.market_type`,
          underlying: sql`excluded.underlying`,
          contractSize: sql`excluded.contract_size`,
          contractType: sql`excluded.contract_type`,
          markPrice: sql`excluded.mark_price`,
          indexPrice: sql`excluded.index_price`,
          fundingRate: sql`excluded.funding_rate`,
          fundingRatePrediction: sql`excluded.funding_rate_prediction`,
          openInterest: sql`excluded.open_interest`,
          maxPositionSize: sql`excluded.max_position_size`,
          marginLevels: sql`excluded.margin_levels`,
          tradeable: sql`excluded.tradeable`,
          suspended: sql`excluded.suspended`,
          openingDate: sql`excluded.opening_date`,
          feeScheduleUid: sql`excluded.fee_schedule_uid`,
          tags: sql`excluded.tags`,
          lastPrice: sql`excluded.last_price`,
          bid: sql`excluded.bid`,
          ask: sql`excluded.ask`,
          volume24h: sql`excluded.volume_24h`,
          deletedAt: sql`NULL`,
        },
      });
  }

  return pairList.length;
}

/**
 * Soft delete pairs not in the provided list for a given exchange and market_type.
 * Uses temp table approach to avoid PostgreSQL's 65534 parameter limit.
 */
export async function softDeleteMissingPairs(
  exchange: string,
  marketType: string,
  currentPairIds: string[],
): Promise<number> {
  if (currentPairIds.length === 0) return 0;

  const rawSql = getRawSql();

  await rawSql`CREATE TEMP TABLE IF NOT EXISTS temp_current_pairs (pair_id TEXT PRIMARY KEY)`;
  await rawSql`TRUNCATE temp_current_pairs`;

  const BATCH_SIZE = 10000;
  for (let i = 0; i < currentPairIds.length; i += BATCH_SIZE) {
    const batch = currentPairIds.slice(i, i + BATCH_SIZE);
    await rawSql`INSERT INTO temp_current_pairs (pair_id) VALUES ${rawSql(batch.map((t) => [t]))} ON CONFLICT DO NOTHING`;
  }

  const result = await rawSql`
    UPDATE pairs
    SET deleted_at = NOW()
    WHERE deleted_at IS NULL
      AND exchange = ${exchange}
      AND market_type = ${marketType}
      AND pair_id NOT IN (SELECT pair_id FROM temp_current_pairs)
    RETURNING pair_id
  `;

  return result.length;
}
