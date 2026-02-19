/**
 * Cross-feed market lookup by IDs.
 * Searches Kalshi markets, Kraken pairs, and Polymarket conditions/tokens.
 */
import { eq, inArray, isNull, sql } from "drizzle-orm";
import { type Database } from "./client.ts";
import {
  markets,
  events,
  pairs,
  polymarketConditions,
  polymarketTokens,
} from "./schema.ts";

export interface LookupResult {
  id: string;
  feed: string;
  name: string;
  event: string | null;
  series: string | null;
  status: string;
}

const VALID_FEEDS = ["kalshi", "kraken-futures", "polymarket"];

/**
 * Lookup markets across feeds by ID.
 * Searches all feeds in parallel, returning unified results.
 *
 * - Kalshi: matches on markets.ticker
 * - Kraken: matches on pairs.pair_id
 * - Polymarket: matches on polymarket_conditions.condition_id or polymarket_tokens.token_id
 */
export async function lookupMarketsByIds(
  db: Database,
  ids: string[],
  feed?: string,
): Promise<LookupResult[]> {
  if (ids.length === 0) return [];

  const results: LookupResult[] = [];
  const lookups: Promise<void>[] = [];

  // Kalshi: lookup by market ticker, join events for hierarchy
  if (!feed || feed === "kalshi") {
    lookups.push(
      (async () => {
        const rows = await db
          .select({
            ticker: markets.ticker,
            title: markets.title,
            status: markets.status,
            eventTicker: markets.eventTicker,
            seriesTicker: events.seriesTicker,
          })
          .from(markets)
          .innerJoin(events, eq(markets.eventTicker, events.eventTicker))
          .where(
            sql.join(
              [inArray(markets.ticker, ids), isNull(markets.deletedAt)],
              sql` AND `,
            ),
          );

        for (const row of rows) {
          results.push({
            id: row.ticker,
            feed: "kalshi",
            name: row.title,
            event: row.eventTicker,
            series: row.seriesTicker,
            status: row.status,
          });
        }
      })(),
    );
  }

  // Kraken: lookup by pair_id
  if (!feed || feed === "kraken-futures") {
    lookups.push(
      (async () => {
        const rows = await db
          .select({
            pairId: pairs.pairId,
            base: pairs.base,
            quote: pairs.quote,
            status: pairs.status,
            marketType: pairs.marketType,
          })
          .from(pairs)
          .where(
            sql.join(
              [inArray(pairs.pairId, ids), isNull(pairs.deletedAt)],
              sql` AND `,
            ),
          );

        for (const row of rows) {
          results.push({
            id: row.pairId,
            feed: "kraken-futures",
            name: `${row.base}/${row.quote}`,
            event: null,
            series: row.marketType,
            status: row.status ?? "active",
          });
        }
      })(),
    );
  }

  // Polymarket: lookup by condition_id or token_id
  if (!feed || feed === "polymarket") {
    lookups.push(
      (async () => {
        // Check conditions
        const condRows = await db
          .select({
            conditionId: polymarketConditions.conditionId,
            question: polymarketConditions.question,
            status: polymarketConditions.status,
            category: polymarketConditions.category,
          })
          .from(polymarketConditions)
          .where(
            sql.join(
              [
                inArray(polymarketConditions.conditionId, ids),
                isNull(polymarketConditions.deletedAt),
              ],
              sql` AND `,
            ),
          );

        for (const row of condRows) {
          results.push({
            id: row.conditionId,
            feed: "polymarket",
            name: row.question,
            event: null,
            series: row.category,
            status: row.status,
          });
        }

        // Check tokens (asset_id from CLOB)
        const tokenRows = await db
          .select({
            tokenId: polymarketTokens.tokenId,
            conditionId: polymarketTokens.conditionId,
            outcome: polymarketTokens.outcome,
            question: polymarketConditions.question,
            status: polymarketConditions.status,
            category: polymarketConditions.category,
          })
          .from(polymarketTokens)
          .innerJoin(
            polymarketConditions,
            eq(polymarketTokens.conditionId, polymarketConditions.conditionId),
          )
          .where(
            sql.join(
              [
                inArray(polymarketTokens.tokenId, ids),
                isNull(polymarketConditions.deletedAt),
              ],
              sql` AND `,
            ),
          );

        for (const row of tokenRows) {
          results.push({
            id: row.tokenId,
            feed: "polymarket",
            name: `${row.question} [${row.outcome}]`,
            event: row.conditionId,
            series: row.category,
            status: row.status,
          });
        }
      })(),
    );
  }

  await Promise.all(lookups);

  return results;
}

export { VALID_FEEDS };
