/**
 * Series fees database operations with time-travel support
 */
import type postgres from "postgres";
import type { SeriesFee, SeriesFeeChange } from "../types/fee.ts";

/**
 * Result of fee sync operation
 */
export interface FeeSyncResult {
  fetched: number;
  inserted: number;
  skipped: number;
}

/**
 * Upsert fee changes, closing previous periods when new ones start.
 * Deduplicates by source_id to avoid re-inserting the same fee change.
 */
export async function upsertFeeChanges(
  sql: ReturnType<typeof postgres>,
  changes: SeriesFeeChange[]
): Promise<FeeSyncResult> {
  const result: FeeSyncResult = {
    fetched: changes.length,
    inserted: 0,
    skipped: 0,
  };

  for (const change of changes) {
    // Check if already exists by source_id
    const existing = await sql`
      SELECT 1 FROM series_fees WHERE source_id = ${change.id}
    `;

    if (existing.length > 0) {
      result.skipped++;
      continue;
    }

    // Close previous open period for this series
    const closed = await sql`
      UPDATE series_fees
      SET effective_to = ${change.scheduled_ts}
      WHERE series_ticker = ${change.series_ticker}
        AND effective_to IS NULL
        AND effective_from < ${change.scheduled_ts}
    `;

    if (closed.count > 0) {
      console.log(
        `  [DB] Closed previous fee period for ${change.series_ticker}`
      );
    }

    // Insert new fee change
    await sql`
      INSERT INTO series_fees
        (series_ticker, fee_type, fee_multiplier, effective_from, source_id)
      VALUES
        (${change.series_ticker}, ${change.fee_type}::fee_type,
         ${change.fee_multiplier}, ${change.scheduled_ts}, ${change.id})
    `;

    result.inserted++;
  }

  return result;
}

/**
 * Get the current fee schedule for a series (effective_to IS NULL).
 */
export async function getCurrentFee(
  sql: ReturnType<typeof postgres>,
  seriesTicker: string
): Promise<SeriesFee | null> {
  const rows = await sql`
    SELECT id, series_ticker, fee_type, fee_multiplier,
           effective_from, effective_to, source_id, created_at
    FROM series_fees
    WHERE series_ticker = ${seriesTicker}
      AND effective_to IS NULL
    ORDER BY effective_from DESC
    LIMIT 1
  `;

  if (rows.length === 0) {
    return null;
  }

  return rowToSeriesFee(rows[0]);
}

/**
 * Get the fee schedule for a series at a specific point in time.
 */
export async function getFeeAsOf(
  sql: ReturnType<typeof postgres>,
  seriesTicker: string,
  asOf: Date
): Promise<SeriesFee | null> {
  const rows = await sql`
    SELECT id, series_ticker, fee_type, fee_multiplier,
           effective_from, effective_to, source_id, created_at
    FROM series_fees
    WHERE series_ticker = ${seriesTicker}
      AND effective_from <= ${asOf}
      AND (effective_to IS NULL OR effective_to > ${asOf})
    ORDER BY effective_from DESC
    LIMIT 1
  `;

  if (rows.length === 0) {
    return null;
  }

  return rowToSeriesFee(rows[0]);
}

/**
 * List all current fee schedules (for debugging/admin).
 */
export async function listCurrentFees(
  sql: ReturnType<typeof postgres>,
  limit = 100
): Promise<SeriesFee[]> {
  const rows = await sql`
    SELECT id, series_ticker, fee_type, fee_multiplier,
           effective_from, effective_to, source_id, created_at
    FROM series_fees
    WHERE effective_to IS NULL
    ORDER BY series_ticker
    LIMIT ${limit}
  `;

  return rows.map(rowToSeriesFee);
}

/**
 * Get fee sync statistics.
 */
export async function getFeeStats(
  sql: ReturnType<typeof postgres>
): Promise<{ total: number; active: number; historical: number }> {
  const rows = await sql`
    SELECT
      COUNT(*) as total,
      COUNT(*) FILTER (WHERE effective_to IS NULL) as active,
      COUNT(*) FILTER (WHERE effective_to IS NOT NULL) as historical
    FROM series_fees
  `;

  const row = rows[0] as Record<string, number>;
  return {
    total: Number(row.total),
    active: Number(row.active),
    historical: Number(row.historical),
  };
}

/**
 * Convert database row to SeriesFee type.
 */
function rowToSeriesFee(row: Record<string, unknown>): SeriesFee {
  return {
    id: row.id as number,
    series_ticker: row.series_ticker as string,
    fee_type: row.fee_type as SeriesFee["fee_type"],
    fee_multiplier: Number(row.fee_multiplier),
    effective_from: new Date(row.effective_from as string),
    effective_to: row.effective_to ? new Date(row.effective_to as string) : null,
    source_id: row.source_id as string | null,
    created_at: new Date(row.created_at as string),
  };
}
