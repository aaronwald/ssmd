/**
 * Binance secmaster sync command - sync spot pairs to PostgreSQL
 */
import { getDb, closeDb } from "../../lib/db/client.ts";
import { upsertSpotPairs, softDeleteMissingPairs, updateBinanceUsTradeable, markKrakenUsTradeable } from "../../lib/db/pairs.ts";
import type { NewPair } from "../../lib/db/schema.ts";

// Use data-api.binance.vision — api.binance.com returns 451 from US IPs in GKE
const BINANCE_EXCHANGE_INFO_URL = "https://data-api.binance.vision/api/v3/exchangeInfo";
// Binance.US has a separate API with only US-tradeable symbols listed
const BINANCE_US_EXCHANGE_INFO_URL = "https://api.binance.us/api/v3/exchangeInfo";
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

  // Build pair rows — upsert TRADING as active, BREAK as halted
  const pairsToUpsert: NewPair[] = [];
  const allPairIds: string[] = [];

  for (const symbol of usdtSymbols) {
    const pairId = `binance:${symbol.symbol}`;
    allPairIds.push(pairId);

    pairsToUpsert.push({
      pairId,
      exchange: "binance",
      base: symbol.baseAsset,
      quote: symbol.quoteAsset,
      wsName: symbol.symbol.toLowerCase(),
      status: symbol.status === "TRADING" ? "active" : "halted",
      marketType: "spot",
      pairDecimals: symbol.quotePrecision ?? null,
      lotDecimals: symbol.baseAssetPrecision ?? null,
      altname: symbol.symbol,
    });
  }

  const tradingCount = usdtSymbols.filter((s) => s.status === "TRADING").length;
  const breakCount = usdtSymbols.filter((s) => s.status === "BREAK").length;
  console.log(
    `[Binance Spot] Fetched ${result.fetched} symbols, ${result.usdtPairs} USDT pairs (${tradingCount} TRADING, ${breakCount} BREAK)`,
  );

  if (dryRun) {
    console.log("[Binance Spot] Dry run — skipping upsert");
    return result;
  }

  const db = getDb();
  try {
    result.upserted = await upsertSpotPairs(db, pairsToUpsert);
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
 * Sync US tradability by fetching Binance.US exchangeInfo and diffing against global pairs.
 * Also marks all active Kraken spot pairs as US-tradeable (GKE IP already filters).
 */
async function runSyncUs(dryRun: boolean): Promise<void> {
  console.log("\n[Binance.US] Fetching exchangeInfo...");
  const res = await fetch(BINANCE_US_EXCHANGE_INFO_URL, {
    signal: AbortSignal.timeout(API_TIMEOUT_MS),
  });
  if (!res.ok) {
    throw new Error(`Binance.US API error: ${res.status} ${await res.text()}`);
  }
  const data: BinanceExchangeInfo = await res.json();

  const usSymbols = new Set<string>();
  for (const s of data.symbols) {
    if (s.quoteAsset === "USDT" && s.status === "TRADING") {
      usSymbols.add(s.symbol);
    }
  }

  console.log(`[Binance.US] Found ${usSymbols.size} US-tradeable USDT pairs`);

  if (dryRun) {
    console.log("[Binance.US] Dry run — not updating database");
    console.log("US-tradeable symbols:", [...usSymbols].sort().join(", "));
    return;
  }

  const { marked, restricted } = await updateBinanceUsTradeable(usSymbols);
  const krakenCount = await markKrakenUsTradeable();

  console.log(`\n=== US Tradability Summary ===`);
  console.log(`Binance: ${marked} US-tradeable, ${restricted} US-restricted`);
  console.log(`Kraken: ${krakenCount} marked US-tradeable (GKE IP-filtered)`);

  await closeDb();
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

    case "sync-us": {
      try {
        await runSyncUs(Boolean(flags["dry-run"]));
      } catch (e) {
        console.error(`Binance US sync failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    default:
      console.log("Usage: ssmd binance <command>");
      console.log();
      console.log("Commands:");
      console.log("  sync         Sync Binance spot pairs (USDT quote)");
      console.log("  sync-us      Sync US tradability from Binance.US API");
      console.log();
      console.log("Options:");
      console.log("  --no-delete  Skip soft-deleting missing pairs (sync only)");
      console.log("  --dry-run    Fetch but don't write to database");
      Deno.exit(1);
  }
}
