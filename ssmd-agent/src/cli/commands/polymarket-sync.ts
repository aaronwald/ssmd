/**
 * Polymarket secmaster sync command - sync conditions and tokens to PostgreSQL
 */
import { getDb, closeDb } from "../../lib/db/client.ts";
import {
  upsertConditions,
  upsertTokens,
  softDeleteMissingConditions,
} from "../../lib/db/polymarket.ts";
import type { NewPolymarketCondition, NewPolymarketToken } from "../../lib/db/schema.ts";

const GAMMA_API_URL = "https://gamma-api.polymarket.com";
const PAGE_SIZE = 100;
const API_TIMEOUT_MS = 30000;

// --- Gamma API response types ---

interface GammaTag {
  id: string;
  label: string;
  slug: string;
}

interface GammaEventMarket {
  id?: string;
  conditionId?: string;
  questionID?: string;
  question?: string;
  slug?: string;
  category?: string;
  outcomes?: string;
  outcomePrices?: string;
  clobTokenIds?: string;
  active?: boolean;
  closed?: boolean;
  endDate?: string;
  resolutionDate?: string;
  winningOutcome?: string;
  volume?: string;
  liquidity?: string;
}

interface GammaEvent {
  id: string;
  title?: string;
  slug?: string;
  category?: string;
  tags?: GammaTag[];
  active?: boolean;
  closed?: boolean;
  markets?: GammaEventMarket[];
}

// --- Parsing helpers ---

/**
 * Parse a stringified JSON array from the Gamma API.
 * The API returns fields like `"[\"id1\",\"id2\"]"` instead of native arrays.
 */
function parseStringifiedArray(value: string | undefined): string[] {
  if (!value || value === "[]" || value === "") return [];
  try {
    const parsed = JSON.parse(value);
    if (Array.isArray(parsed)) return parsed;
    return [];
  } catch {
    return [];
  }
}

/**
 * Map Gamma API status fields to our internal status.
 */
function mapStatus(active: boolean | undefined, closed: boolean | undefined): string {
  if (closed) return "resolved";
  if (active === false) return "inactive";
  return "active";
}

// --- Sync ---

export interface PolymarketSyncOptions {
  noDelete?: boolean;
  dryRun?: boolean;
}

export interface PolymarketSyncResult {
  fetched: number;
  conditionsUpserted: number;
  tokensUpserted: number;
  deleted: number;
}

/**
 * Stream active events from the Gamma API in paginated batches.
 * Events include tags and nested markets arrays.
 */
async function* fetchGammaEventBatches(): AsyncGenerator<GammaEvent[]> {
  let offset = 0;

  while (true) {
    const url = `${GAMMA_API_URL}/events?active=true&closed=false&limit=${PAGE_SIZE}&offset=${offset}`;
    const res = await fetch(url, { signal: AbortSignal.timeout(API_TIMEOUT_MS) });

    if (!res.ok) {
      throw new Error(`Gamma API error: ${res.status} ${await res.text()}`);
    }

    const events: GammaEvent[] = await res.json();
    if (events.length === 0) break;

    yield events;

    if (events.length < PAGE_SIZE) break;
    offset += PAGE_SIZE;
  }
}

/**
 * Run Polymarket sync
 */
export async function runPolymarketSync(
  options: PolymarketSyncOptions = {},
): Promise<PolymarketSyncResult> {
  const syncStartMs = Date.now();
  const noDelete = options.noDelete ?? false;
  const dryRun = options.dryRun ?? false;

  console.log("\n[Polymarket] Fetching active events...");

  const result: PolymarketSyncResult = {
    fetched: 0,
    conditionsUpserted: 0,
    tokensUpserted: 0,
    deleted: 0,
  };

  let eventCount = 0;

  // Well-known tag labels for category assignment (priority order)
  const WELL_KNOWN_TAGS = [
    "Crypto", "Sports", "Politics", "Science & Tech",
    "Finance", "Business", "Pop Culture", "Entertainment",
  ];

  // Track IDs seen during this run (used for delete + dry-run summary)
  const seenConditionIds = new Set<string>();
  const dryRunConditionIds = new Set<string>();
  const dryRunTokenIds = new Set<string>();
  const dryRunConditionCategoryById = new Map<string, string | null>();

  const db = dryRun ? null : getDb();

  let pageCount = 0;
  let skipped = 0;
  let totalProcessingMs = 0;
  let totalFetchWaitMs = 0;
  let totalDbUpsertMs = 0;

  try {
    const eventBatchIterator = fetchGammaEventBatches()[Symbol.asyncIterator]();
    let nextBatch = eventBatchIterator.next();

    while (true) {
      const fetchWaitStartMs = Date.now();
      const currentBatch = await nextBatch;
      totalFetchWaitMs += Date.now() - fetchWaitStartMs;
      if (currentBatch.done) {
        break;
      }

      const events = currentBatch.value;
      nextBatch = eventBatchIterator.next();
      nextBatch.catch(() => {}); // Prevent unhandled rejection if we exit early
      const pageProcessStartMs = Date.now();

      pageCount++;
      eventCount += events.length;

      // Per-page dedup; DB handles cross-page dedup via ON CONFLICT.
      const conditionMap = new Map<string, NewPolymarketCondition>();
      const tokenMap = new Map<string, NewPolymarketToken>();

      for (const event of events) {
        const tags = event.tags ?? [];
        const tagSlugs = tags.map((t) => t.slug);

        // Pick category from well-known tags (first match wins)
        let category: string | null = null;
        for (const wk of WELL_KNOWN_TAGS) {
          if (tags.some((t) => t.label === wk)) {
            category = wk;
            break;
          }
        }
        // Fallback: use first tag label if no well-known match
        if (!category && tags.length > 0) {
          category = tags[0].label;
        }

        for (const market of event.markets ?? []) {
          result.fetched++;

          const conditionId = market.conditionId ?? market.questionID;
          if (!conditionId) {
            skipped++;
            continue;
          }

          const tokenIds = parseStringifiedArray(market.clobTokenIds);
          if (tokenIds.length === 0) {
            skipped++;
            continue;
          }

          const outcomes = parseStringifiedArray(market.outcomes);
          const outcomePrices = parseStringifiedArray(market.outcomePrices);
          const status = mapStatus(market.active, market.closed);

          seenConditionIds.add(conditionId);

          // Upsert condition (dedup: last occurrence wins)
          conditionMap.set(conditionId, {
            conditionId,
            question: market.question ?? "",
            slug: market.slug ?? null,
            category,
            tags: tagSlugs,
            outcomes,
            status,
            active: market.active ?? true,
            endDate: market.endDate ? new Date(market.endDate) : null,
            resolutionDate: market.resolutionDate ? new Date(market.resolutionDate) : null,
            winningOutcome: market.winningOutcome ?? null,
            volume: market.volume ?? null,
            liquidity: market.liquidity ?? null,
          });

          if (dryRun) {
            dryRunConditionIds.add(conditionId);
            dryRunConditionCategoryById.set(conditionId, category);
          }

          // Map outcomes 1:1 to token IDs (dedup: last occurrence wins)
          for (let i = 0; i < tokenIds.length; i++) {
            const tokenId = tokenIds[i];
            const outcome = outcomes[i] ?? (i === 0 ? "Yes" : "No");
            const price = outcomePrices[i] ?? null;

            if (dryRun) {
              dryRunTokenIds.add(tokenId);
              continue;
            }

            tokenMap.set(tokenId, {
              tokenId,
              conditionId,
              outcome,
              outcomeIndex: i,
              price,
            });
          }
        }
      }

      if (db) {
        const conditions = Array.from(conditionMap.values());
        const tokens = Array.from(tokenMap.values());
        const dbUpsertStartMs = Date.now();

        result.conditionsUpserted += await upsertConditions(db, conditions);
        result.tokensUpserted += await upsertTokens(db, tokens);
        const dbUpsertDurationMs = Date.now() - dbUpsertStartMs;
        totalDbUpsertMs += dbUpsertDurationMs;

        const pageDurationMs = Date.now() - pageProcessStartMs;
        totalProcessingMs += pageDurationMs;
        console.log(
          `[Polymarket] Page ${pageCount}: ${events.length} events, ${conditions.length} conditions, ` +
            `${tokens.length} tokens (${pageDurationMs}ms, db ${dbUpsertDurationMs}ms)`,
        );
      } else {
        const pageDurationMs = Date.now() - pageProcessStartMs;
        totalProcessingMs += pageDurationMs;
        console.log(
          `[Polymarket] Page ${pageCount}: ${events.length} events processed (${pageDurationMs}ms, dry-run)`,
        );
      }
    }

    // Soft-delete must run inside try block while DB is still open
    if (!noDelete && !dryRun) {
      result.deleted = await softDeleteMissingConditions(Array.from(seenConditionIds));
      if (result.deleted > 0) {
        console.log(`[Polymarket] Soft-deleted ${result.deleted} missing conditions`);
      }
    }
  } finally {
    if (db) {
      await closeDb();
    }
  }

  console.log(`[Polymarket] Fetched ${result.fetched} markets from ${eventCount} events in ${pageCount} pages`);

  if (skipped > 0) {
    console.log(`[Polymarket] Skipped ${skipped} markets (missing conditionId or tokenIds)`);
  }

  if (dryRun) {
    console.log(
      `[Polymarket] Dry run — would upsert ${dryRunConditionIds.size} conditions, ${dryRunTokenIds.size} tokens`,
    );

    // Show tag distribution
    const tagCounts = new Map<string, number>();
    for (const category of dryRunConditionCategoryById.values()) {
      const cat = category ?? "uncategorized";
      tagCounts.set(cat, (tagCounts.get(cat) ?? 0) + 1);
    }
    for (const [tag, count] of [...tagCounts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 10)) {
      console.log(`  ${tag}: ${count} conditions`);
    }

    return result;
  }

  console.log(`[Polymarket] Upserted ${result.conditionsUpserted} conditions`);
  console.log(`[Polymarket] Upserted ${result.tokensUpserted} tokens`);

  console.log(`\n=== Polymarket Sync Summary ===`);
  console.log(`Events:     ${eventCount} fetched`);
  console.log(`Conditions: ${result.conditionsUpserted} upserted, ${result.deleted} deleted`);
  console.log(`Tokens:     ${result.tokensUpserted} upserted`);
  console.log(
    `Timing:     total ${Date.now() - syncStartMs}ms, fetch-wait ${totalFetchWaitMs}ms, processing ${totalProcessingMs}ms` +
      `, db-upsert ${totalDbUpsertMs}ms`,
  );

  return result;
}

/**
 * Handle `ssmd polymarket` subcommands
 */
export async function handlePolymarket(
  subcommand: string,
  flags: Record<string, unknown>,
): Promise<void> {
  switch (subcommand) {
    case "sync": {
      const options: PolymarketSyncOptions = {
        noDelete: Boolean(flags["no-delete"]),
        dryRun: Boolean(flags["dry-run"]),
      };

      try {
        await runPolymarketSync(options);
      } catch (e) {
        console.error(`Polymarket sync failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    default:
      console.log("Usage: ssmd polymarket <command>");
      console.log();
      console.log("Commands:");
      console.log("  sync         Sync Polymarket conditions and tokens");
      console.log();
      console.log("Options for sync:");
      console.log("  --no-delete  Skip soft-deleting missing conditions");
      console.log("  --dry-run    Fetch but don't write to database");
      Deno.exit(1);
  }
}
