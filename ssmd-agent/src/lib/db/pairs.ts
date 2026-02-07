/**
 * Pairs database operations for Kraken spot + perpetual upserts (Drizzle ORM)
 */
import { eq, isNull, desc, sql, count } from "drizzle-orm";
import { type Database, getRawSql } from "./client.ts";
import { pairs, pairSnapshots, type NewPair, type NewPairSnapshot, type Pair, type PairSnapshot } from "./schema.ts";

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

/**
 * List pairs with optional filters.
 */
export async function listPairs(
  db: Database,
  options: {
    exchange?: string;
    marketType?: string;
    base?: string;
    quote?: string;
    status?: string;
    limit?: number;
  } = {},
): Promise<Pair[]> {
  const limit = options.limit ?? 100;

  const conditions: ReturnType<typeof sql>[] = [
    isNull(pairs.deletedAt),
  ];

  if (options.exchange) {
    conditions.push(eq(pairs.exchange, options.exchange));
  }
  if (options.marketType) {
    conditions.push(eq(pairs.marketType, options.marketType));
  }
  if (options.base) {
    conditions.push(sql`UPPER(${pairs.base}) = UPPER(${options.base})`);
  }
  if (options.quote) {
    conditions.push(sql`UPPER(${pairs.quote}) = UPPER(${options.quote})`);
  }
  if (options.status) {
    conditions.push(eq(pairs.status, options.status));
  }

  return await db
    .select()
    .from(pairs)
    .where(sql.join(conditions, sql` AND `))
    .orderBy(desc(pairs.updatedAt))
    .limit(limit);
}

/**
 * Get a single pair by namespaced ID.
 */
export async function getPair(
  db: Database,
  pairId: string,
): Promise<Pair | null> {
  const rows = await db
    .select()
    .from(pairs)
    .where(
      sql`${eq(pairs.pairId, pairId)} AND ${isNull(pairs.deletedAt)}`,
    );

  return rows.length > 0 ? rows[0] : null;
}

/**
 * Get pair statistics (counts by exchange and market_type).
 */
export async function getPairStats(
  db: Database,
): Promise<{
  total: number;
  by_exchange: Record<string, number>;
  by_market_type: Record<string, number>;
}> {
  const exchangeRows = await db
    .select({
      exchange: pairs.exchange,
      count: count(),
    })
    .from(pairs)
    .where(isNull(pairs.deletedAt))
    .groupBy(pairs.exchange);

  const marketTypeRows = await db
    .select({
      marketType: pairs.marketType,
      count: count(),
    })
    .from(pairs)
    .where(isNull(pairs.deletedAt))
    .groupBy(pairs.marketType);

  const by_exchange: Record<string, number> = {};
  let total = 0;
  for (const row of exchangeRows) {
    by_exchange[row.exchange] = row.count;
    total += row.count;
  }

  const by_market_type: Record<string, number> = {};
  for (const row of marketTypeRows) {
    by_market_type[row.marketType] = row.count;
  }

  return { total, by_exchange, by_market_type };
}

const SNAPSHOTS_BATCH_SIZE = 500;

/**
 * Batch insert pair snapshot rows from perpetual sync.
 * Records a point-in-time snapshot of mark price, funding rate, etc.
 */
export async function insertPerpSnapshots(
  db: Database,
  perpPairs: NewPair[],
): Promise<number> {
  if (perpPairs.length === 0) return 0;

  const snapshots: NewPairSnapshot[] = perpPairs
    .filter((p) => p.markPrice != null || p.fundingRate != null || p.lastPrice != null)
    .map((p) => ({
      pairId: p.pairId,
      markPrice: p.markPrice ?? null,
      indexPrice: p.indexPrice ?? null,
      fundingRate: p.fundingRate ?? null,
      fundingRatePrediction: p.fundingRatePrediction ?? null,
      openInterest: p.openInterest ?? null,
      lastPrice: p.lastPrice ?? null,
      bid: p.bid ?? null,
      ask: p.ask ?? null,
      volume24h: p.volume24h ?? null,
      suspended: p.suspended ?? false,
    }));

  if (snapshots.length === 0) return 0;

  for (let i = 0; i < snapshots.length; i += SNAPSHOTS_BATCH_SIZE) {
    const chunk = snapshots.slice(i, i + SNAPSHOTS_BATCH_SIZE);
    await db.insert(pairSnapshots).values(chunk);
  }

  return snapshots.length;
}

/**
 * Get time-series snapshots for a pair.
 */
export async function getPairSnapshots(
  db: Database,
  pairId: string,
  options: {
    from?: string;
    to?: string;
    limit?: number;
  } = {},
): Promise<PairSnapshot[]> {
  const limit = options.limit ?? 100;

  const conditions: ReturnType<typeof sql>[] = [
    eq(pairSnapshots.pairId, pairId),
  ];

  if (options.from) {
    conditions.push(sql`${pairSnapshots.snapshotAt} >= ${options.from}`);
  }
  if (options.to) {
    conditions.push(sql`${pairSnapshots.snapshotAt} <= ${options.to}`);
  }

  return await db
    .select()
    .from(pairSnapshots)
    .where(sql.join(conditions, sql` AND `))
    .orderBy(desc(pairSnapshots.snapshotAt))
    .limit(limit);
}

/**
 * Delete pair snapshots older than the retention period.
 * Called after each sync to keep the table bounded.
 */
export async function cleanupOldSnapshots(
  retentionDays = 7,
): Promise<number> {
  const rawSql = getRawSql();
  const result = await rawSql`
    DELETE FROM pair_snapshots
    WHERE snapshot_at < NOW() - make_interval(days => ${retentionDays})
    RETURNING id
  `;
  return result.length;
}
