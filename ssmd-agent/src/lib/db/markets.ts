/**
 * Market database operations with bulk upsert support
 */
import type postgres from "postgres";
import type { Market } from "../types/market.ts";
import { getExistingEventTickers } from "./events.ts";

const BATCH_SIZE = 500;

export interface MarketBulkResult {
  batches: number;
  total: number;
  skipped: number;
}

/**
 * Bulk upsert markets with 500-row batches.
 * Pre-filters by existing events to avoid FK violations.
 */
export async function bulkUpsertMarkets(
  sql: ReturnType<typeof postgres>,
  markets: Market[]
): Promise<MarketBulkResult> {
  if (markets.length === 0) {
    return { batches: 0, total: 0, skipped: 0 };
  }

  // Collect unique event tickers
  const eventTickers = [...new Set(markets.map((m) => m.event_ticker))];

  // Pre-filter by existing events (FK constraint)
  const existingEvents = await getExistingEventTickers(sql, eventTickers);
  console.log(
    `  [DB] found ${existingEvents.size}/${eventTickers.length} parent events`
  );

  // Filter markets to only those with existing parent events
  const validMarkets = markets.filter((m) => existingEvents.has(m.event_ticker));
  const skipped = markets.length - validMarkets.length;

  if (skipped > 0) {
    console.log(`  [DB] skipping ${skipped} markets with missing events`);
  }

  if (validMarkets.length === 0) {
    return { batches: 0, total: 0, skipped };
  }

  let batches = 0;

  for (let i = 0; i < validMarkets.length; i += BATCH_SIZE) {
    const batch = validMarkets.slice(i, i + BATCH_SIZE);

    await sql`
      INSERT INTO markets ${sql(
        batch,
        "ticker",
        "event_ticker",
        "title",
        "status",
        "close_time",
        "yes_bid",
        "yes_ask",
        "no_bid",
        "no_ask",
        "last_price",
        "volume",
        "volume_24h",
        "open_interest"
      )}
      ON CONFLICT (ticker) DO UPDATE SET
        event_ticker = EXCLUDED.event_ticker,
        title = EXCLUDED.title,
        status = EXCLUDED.status,
        close_time = EXCLUDED.close_time,
        yes_bid = EXCLUDED.yes_bid,
        yes_ask = EXCLUDED.yes_ask,
        no_bid = EXCLUDED.no_bid,
        no_ask = EXCLUDED.no_ask,
        last_price = EXCLUDED.last_price,
        volume = EXCLUDED.volume,
        volume_24h = EXCLUDED.volume_24h,
        open_interest = EXCLUDED.open_interest,
        updated_at = NOW(),
        deleted_at = NULL
    `;

    batches++;
    console.log(`  [DB] markets batch ${batches}: ${batch.length} upserted`);
  }

  return { batches, total: validMarkets.length, skipped };
}

/**
 * Soft delete markets that are no longer in the API response.
 */
export async function softDeleteMissingMarkets(
  sql: ReturnType<typeof postgres>,
  currentTickers: string[]
): Promise<number> {
  const result = await sql`
    UPDATE markets
    SET deleted_at = NOW()
    WHERE ticker != ALL(${currentTickers})
    AND deleted_at IS NULL
  `;

  return result.count;
}
