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

// --- Gamma API response type ---

interface GammaMarket {
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
  acceptingOrders?: boolean;
  endDate?: string;
  resolutionDate?: string;
  winningOutcome?: string;
  volume?: string;
  liquidity?: string;
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
 * Fetch all active markets from the Gamma API with pagination.
 */
async function fetchAllGammaMarkets(): Promise<GammaMarket[]> {
  const allMarkets: GammaMarket[] = [];
  let offset = 0;

  while (true) {
    const url = `${GAMMA_API_URL}/markets?active=true&closed=false&limit=${PAGE_SIZE}&offset=${offset}`;
    const res = await fetch(url, { signal: AbortSignal.timeout(API_TIMEOUT_MS) });

    if (!res.ok) {
      throw new Error(`Gamma API error: ${res.status} ${await res.text()}`);
    }

    const markets: GammaMarket[] = await res.json();
    if (markets.length === 0) break;

    allMarkets.push(...markets);

    if (markets.length < PAGE_SIZE) break;
    offset += PAGE_SIZE;
  }

  return allMarkets;
}

/**
 * Run Polymarket sync
 */
export async function runPolymarketSync(
  options: PolymarketSyncOptions = {},
): Promise<PolymarketSyncResult> {
  const noDelete = options.noDelete ?? false;
  const dryRun = options.dryRun ?? false;

  console.log("\n[Polymarket] Fetching active conditions...");

  const gammaMarkets = await fetchAllGammaMarkets();

  const result: PolymarketSyncResult = {
    fetched: gammaMarkets.length,
    conditionsUpserted: 0,
    tokensUpserted: 0,
    deleted: 0,
  };

  console.log(`[Polymarket] Fetched ${result.fetched} active conditions`);

  // Convert to DB rows, dedup by conditionId and tokenId
  const conditionMap = new Map<string, NewPolymarketCondition>();
  const tokenMap = new Map<string, NewPolymarketToken>();
  let skipped = 0;

  for (const market of gammaMarkets) {
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

    // Upsert condition (dedup: last occurrence wins)
    conditionMap.set(conditionId, {
      conditionId,
      question: market.question ?? "",
      slug: market.slug ?? null,
      category: market.category ?? null,
      outcomes,
      status,
      active: market.active ?? true,
      endDate: market.endDate ? new Date(market.endDate) : null,
      resolutionDate: market.resolutionDate ? new Date(market.resolutionDate) : null,
      winningOutcome: market.winningOutcome ?? null,
      volume: market.volume ?? null,
      liquidity: market.liquidity ?? null,
    });

    // Map outcomes 1:1 to token IDs (dedup: last occurrence wins)
    for (let i = 0; i < tokenIds.length; i++) {
      const outcome = outcomes[i] ?? (i === 0 ? "Yes" : "No");
      const price = outcomePrices[i] ?? null;

      tokenMap.set(tokenIds[i], {
        tokenId: tokenIds[i],
        conditionId,
        outcome,
        outcomeIndex: i,
        price,
      });
    }
  }

  if (skipped > 0) {
    console.log(`[Polymarket] Skipped ${skipped} markets (missing conditionId or tokenIds)`);
  }

  const conditions = Array.from(conditionMap.values());
  const allTokens = Array.from(tokenMap.values());

  if (dryRun) {
    console.log(`[Polymarket] Dry run — would upsert ${conditions.length} conditions, ${allTokens.length} tokens`);
    return result;
  }

  const db = getDb();

  try {
    // Upsert conditions first (FK constraint for tokens)
    result.conditionsUpserted = await upsertConditions(db, conditions);
    console.log(`[Polymarket] Upserted ${result.conditionsUpserted} conditions`);

    // Then upsert tokens
    result.tokensUpserted = await upsertTokens(db, allTokens);
    console.log(`[Polymarket] Upserted ${result.tokensUpserted} tokens`);

    // Soft-delete conditions not in the active set. Since we only fetch
    // active=true&closed=false, resolved/inactive conditions will be marked
    // deleted_at on every sync. This is intentional: the active universe is
    // what matters for connector subscriptions and arb signal evaluation.
    // Historical conditions remain queryable via deleted_at IS NOT NULL.
    if (!noDelete) {
      const currentIds = conditions.map((c) => c.conditionId);
      result.deleted = await softDeleteMissingConditions(currentIds);
      if (result.deleted > 0) {
        console.log(`[Polymarket] Soft-deleted ${result.deleted} missing conditions`);
      }
    }

    console.log(`\n=== Polymarket Sync Summary ===`);
    console.log(`Conditions: ${result.conditionsUpserted} upserted, ${result.deleted} deleted`);
    console.log(`Tokens:     ${result.tokensUpserted} upserted`);

    return result;
  } finally {
    await closeDb();
  }
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
