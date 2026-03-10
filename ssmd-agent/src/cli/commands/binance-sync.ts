/**
 * Binance secmaster sync command - sync spot pairs to PostgreSQL
 */
import { getDb, closeDb } from "../../lib/db/client.ts";
import { upsertSpotPairs, softDeleteMissingPairs } from "../../lib/db/pairs.ts";
import type { NewPair } from "../../lib/db/schema.ts";

// Use data-api.binance.vision — api.binance.com returns 451 from US IPs in GKE
const BINANCE_EXCHANGE_INFO_URL = "https://data-api.binance.vision/api/v3/exchangeInfo";
const API_TIMEOUT_MS = 30000;

// --- Binance API response types ---

interface BinanceExchangeInfo {
  timezone: string;
  serverTime: number;
  symbols: BinanceSymbol[];
}

interface BinanceSymbol {
  symbol: string;
  status: string;
  baseAsset: string;
  quoteAsset: string;
  baseAssetPrecision: number;
  quotePrecision: number;
  quoteAssetPrecision: number;
  orderTypes: string[];
  icebergAllowed: boolean;
  isSpotTradingAllowed: boolean;
  isMarginTradingAllowed: boolean;
  filters: unknown[];
  permissions: string[];
}

// --- Sync functions ---

export interface BinanceSyncOptions {
  noDelete?: boolean;
  dryRun?: boolean;
}

export interface BinanceSyncResult {
  fetched: number;
  usdtPairs: number;
  upserted: number;
  deleted: number;
}

/**
 * Sync Binance spot pairs from the exchangeInfo API.
 */
export async function runBinanceSync(
  options: BinanceSyncOptions = {},
): Promise<BinanceSyncResult> {
  const noDelete = options.noDelete ?? false;
  const dryRun = options.dryRun ?? false;

  const result: BinanceSyncResult = { fetched: 0, usdtPairs: 0, upserted: 0, deleted: 0 };

  console.log("\n[Binance Spot] Fetching exchange info...");
  const res = await fetch(BINANCE_EXCHANGE_INFO_URL, {
    signal: AbortSignal.timeout(API_TIMEOUT_MS),
  });
  if (!res.ok) {
    throw new Error(`Binance API error: ${res.status} ${await res.text()}`);
  }
  const data: BinanceExchangeInfo = await res.json();

  result.fetched = data.symbols.length;

  // Filter to USDT quote pairs (include both TRADING and BREAK for soft-delete handling)
  const usdtSymbols = data.symbols.filter(
    (s) => s.quoteAsset === "USDT" && (s.status === "TRADING" || s.status === "BREAK"),
  );
  result.usdtPairs = usdtSymbols.length;

  // Build pair rows — only upsert TRADING pairs
  const tradingPairs: NewPair[] = [];
  const allPairIds: string[] = [];

  for (const symbol of usdtSymbols) {
    const pairId = `binance:${symbol.symbol}`;
    allPairIds.push(pairId);

    if (symbol.status === "TRADING") {
      tradingPairs.push({
        pairId,
        exchange: "binance",
        base: symbol.baseAsset,
        quote: symbol.quoteAsset,
        wsName: symbol.symbol.toLowerCase(),
        status: "active",
        marketType: "spot",
        pairDecimals: symbol.quotePrecision ?? null,
        lotDecimals: symbol.baseAssetPrecision ?? null,
        altname: symbol.symbol,
      });
    }
  }

  console.log(
    `[Binance Spot] Fetched ${result.fetched} symbols, ${result.usdtPairs} USDT pairs (${tradingPairs.length} TRADING)`,
  );

  if (dryRun) {
    console.log("[Binance Spot] Dry run — skipping upsert");
    return result;
  }

  const db = getDb();
  try {
    result.upserted = await upsertSpotPairs(db, tradingPairs);
    console.log(`[Binance Spot] Upserted ${result.upserted} spot pairs`);

    if (!noDelete) {
      result.deleted = await softDeleteMissingPairs("binance", "spot", allPairIds);
      if (result.deleted > 0) {
        console.log(`[Binance Spot] Soft-deleted ${result.deleted} missing pairs`);
      }
    }

    console.log("\n=== Binance Sync Summary ===");
    console.log(`Spot: ${result.upserted} upserted, ${result.deleted} deleted`);

    return result;
  } finally {
    await closeDb();
  }
}

/**
 * Handle `ssmd binance` subcommands
 */
export async function handleBinance(
  subcommand: string,
  flags: Record<string, unknown>,
): Promise<void> {
  switch (subcommand) {
    case "sync": {
      const options: BinanceSyncOptions = {
        noDelete: Boolean(flags["no-delete"]),
        dryRun: Boolean(flags["dry-run"]),
      };

      try {
        await runBinanceSync(options);
      } catch (e) {
        console.error(`Binance sync failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    default:
      console.log("Usage: ssmd binance <command>");
      console.log();
      console.log("Commands:");
      console.log("  sync         Sync Binance spot pairs (USDT quote)");
      console.log();
      console.log("Options for sync:");
      console.log("  --no-delete  Skip soft-deleting missing pairs");
      console.log("  --dry-run    Fetch but don't write to database");
      Deno.exit(1);
  }
}
