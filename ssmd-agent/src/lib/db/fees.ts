/**
 * Series fees database operations with time-travel support
 * Migrated to Drizzle ORM
 */
import { eq, isNull, desc, and, lte, or, gt, lt, sql, asc } from "drizzle-orm";
import type { Database } from "./client.ts";
import { seriesFees, type SeriesFee } from "./schema.ts";
import type { SeriesFeeChange } from "../types/fee.ts";

/**
 * Result of fee sync operation
 */
export interface FeeSyncResult {
  fetched: number;
  inserted: number;
  skipped: number;
}

/**
 * Options for listing current fees
 */
export interface ListCurrentFeesOptions {
  limit?: number;
}

/**
 * Upsert fee changes, closing previous periods when new ones start.
 * Deduplicates by source_id to avoid re-inserting the same fee change.
 */
export async function upsertFeeChanges(
  db: Database,
  changes: SeriesFeeChange[]
): Promise<FeeSyncResult> {
  const result: FeeSyncResult = {
    fetched: changes.length,
    inserted: 0,
    skipped: 0,
  };

  for (const change of changes) {
    // Check if already exists by source_id
    const existing = await db
      .select({ id: seriesFees.id })
      .from(seriesFees)
      .where(eq(seriesFees.sourceId, change.id))
      .limit(1);

    if (existing.length > 0) {
      result.skipped++;
      continue;
    }

    const scheduledTs = new Date(change.scheduled_ts);

    // Close previous open period for this series
    const closed = await db
      .update(seriesFees)
      .set({ effectiveTo: scheduledTs })
      .where(
        and(
          eq(seriesFees.seriesTicker, change.series_ticker),
          isNull(seriesFees.effectiveTo),
          lt(seriesFees.effectiveFrom, scheduledTs)
        )
      )
      .returning({ seriesTicker: seriesFees.seriesTicker });

    if (closed.length > 0) {
      console.log(
        `  [DB] Closed previous fee period for ${change.series_ticker}`
      );
    }

    // Insert new fee change
    await db.insert(seriesFees).values({
      seriesTicker: change.series_ticker,
      feeType: change.fee_type,
      feeMultiplier: change.fee_multiplier.toString(),
      effectiveFrom: scheduledTs,
      sourceId: change.id,
    });

    result.inserted++;
  }

  return result;
}

/**
 * Get the current fee schedule for a series (effective_to IS NULL).
 */
export async function getCurrentFee(
  db: Database,
  seriesTicker: string
): Promise<SeriesFee | null> {
  const rows = await db
    .select()
    .from(seriesFees)
    .where(
      and(
        eq(seriesFees.seriesTicker, seriesTicker),
        isNull(seriesFees.effectiveTo)
      )
    )
    .orderBy(desc(seriesFees.effectiveFrom))
    .limit(1);

  if (rows.length === 0) {
    return null;
  }

  return rows[0];
}

/**
 * Get the fee schedule for a series at a specific point in time.
 */
export async function getFeeAsOf(
  db: Database,
  seriesTicker: string,
  asOf: Date
): Promise<SeriesFee | null> {
  const rows = await db
    .select()
    .from(seriesFees)
    .where(
      and(
        eq(seriesFees.seriesTicker, seriesTicker),
        lte(seriesFees.effectiveFrom, asOf),
        or(isNull(seriesFees.effectiveTo), gt(seriesFees.effectiveTo, asOf))
      )
    )
    .orderBy(desc(seriesFees.effectiveFrom))
    .limit(1);

  if (rows.length === 0) {
    return null;
  }

  return rows[0];
}

/**
 * List all current fee schedules (for debugging/admin).
 */
export async function listCurrentFees(
  db: Database,
  options: ListCurrentFeesOptions = {}
): Promise<SeriesFee[]> {
  const { limit = 100 } = options;

  const rows = await db
    .select()
    .from(seriesFees)
    .where(isNull(seriesFees.effectiveTo))
    .orderBy(asc(seriesFees.seriesTicker))
    .limit(limit);

  return rows;
}

/**
 * Seed fee records for series that have no fee_changes but do have
 * fee_type/fee_multiplier on their series metadata from the Kalshi API.
 * This fills the gap where series launched with an initial fee schedule
 * and never had a fee change recorded.
 */
export async function seedMissingFees(
  db: Database,
  seriesList: Array<{ ticker: string; fee_type: string; fee_multiplier: number }>
): Promise<{ seeded: number; skipped: number }> {
  let seeded = 0;
  let skipped = 0;

  for (const s of seriesList) {
    // Check if series already has any fee record
    const existing = await db
      .select({ id: seriesFees.id })
      .from(seriesFees)
      .where(eq(seriesFees.seriesTicker, s.ticker))
      .limit(1);

    if (existing.length > 0) {
      skipped++;
      continue;
    }

    // Insert initial fee record with a sentinel effective_from date
    await db.insert(seriesFees).values({
      seriesTicker: s.ticker,
      feeType: s.fee_type,
      feeMultiplier: s.fee_multiplier.toString(),
      effectiveFrom: new Date("2020-01-01T00:00:00Z"),
      sourceId: `seed:${s.ticker}`,
    });

    seeded++;
  }

  return { seeded, skipped };
}

/**
 * Get fee sync statistics.
 */
export async function getFeeStats(
  db: Database
): Promise<{ total: number; active: number; historical: number }> {
  const rows = await db
    .select({
      total: sql<number>`count(*)`,
      active: sql<number>`count(*) filter (where ${seriesFees.effectiveTo} is null)`,
      historical: sql<number>`count(*) filter (where ${seriesFees.effectiveTo} is not null)`,
    })
    .from(seriesFees);

  const row = rows[0];
  return {
    total: Number(row.total),
    active: Number(row.active),
    historical: Number(row.historical),
  };
}
