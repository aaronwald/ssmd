/**
 * Kraken secmaster sync command - sync spot pairs and perpetual contracts to PostgreSQL
 */
import { getDb, closeDb } from "../../lib/db/client.ts";
import { upsertSpotPairs, upsertPerpPairs, softDeleteMissingPairs } from "../../lib/db/pairs.ts";
import type { NewPair } from "../../lib/db/schema.ts";

const KRAKEN_SPOT_URL = "https://api.kraken.com/0/public/AssetPairs";
const KRAKEN_FUTURES_INSTRUMENTS_URL = "https://futures.kraken.com/derivatives/api/v3/instruments";
const KRAKEN_FUTURES_TICKERS_URL = "https://futures.kraken.com/derivatives/api/v3/tickers";
const API_TIMEOUT_MS = 30000;

// --- Kraken API response types ---

interface KrakenSpotResponse {
  error: string[];
  result: Record<string, KrakenAssetPair>;
}

interface KrakenAssetPair {
  altname: string;
  wsname?: string;
  aclass_base: string;
  base: string;
  aclass_quote: string;
  quote: string;
  lot: string;
  pair_decimals: number;
  lot_decimals: number;
  lot_multiplier: number;
  fees: number[][];
  fees_maker?: number[][];
  fee_volume_currency: string;
  margin_call: number;
  margin_stop: number;
  ordermin?: string;
  costmin?: string;
  tick_size?: string;
  status: string;
}

interface KrakenFuturesInstrumentsResponse {
  result: string;
  instruments: KrakenFuturesInstrument[];
}

interface KrakenFuturesInstrument {
  symbol: string;
  type: string;
  underlying: string;
  tickSize: number;
  contractSize: number;
  tradeable: boolean;
  suspended?: boolean;
  openingDate?: string;
  feeScheduleUid?: string;
  marginLevels?: Array<{ contracts: number; initialMargin: number; maintenanceMargin: number }>;
  maxPositionSize?: number;
  tags?: string[];
  contractValueTradePrecision?: number;
}

interface KrakenFuturesTickersResponse {
  result: string;
  tickers: KrakenFuturesTicker[];
}

interface KrakenFuturesTicker {
  symbol: string;
  bid: number;
  ask: number;
  last: number;
  vol24h: number;
  markPrice: number;
  indexPrice?: number;
  fundingRate?: number;
  fundingRatePrediction?: number;
  openInterest?: number;
  suspended?: boolean;
  tag?: string;
  pair?: string;
}

// --- Normalization ---

/**
 * Normalize Kraken base asset name.
 * Strip leading X if 4+ chars (XXBT→XBT), then XBT→BTC.
 */
function normalizeBase(raw: string): string {
  let base = raw;
  if (base.length >= 4 && base.startsWith("X")) {
    base = base.slice(1);
  }
  if (base === "XBT") return "BTC";
  return base;
}

/**
 * Normalize Kraken quote asset name.
 * Strip leading Z if 4+ chars (ZUSD→USD).
 */
function normalizeQuote(raw: string): string {
  let quote = raw;
  if (quote.length >= 4 && quote.startsWith("Z")) {
    quote = quote.slice(1);
  }
  if (quote === "XBT") return "BTC";
  return quote;
}

/**
 * Parse Kraken base from perpetual symbol.
 * PF_XBTUSD → BTC, PF_ETHUSD → ETH, PI_XBTUSD → BTC
 */
function parsePerpBase(symbol: string): string {
  // Remove prefix (PF_, PI_, etc.)
  const parts = symbol.split("_");
  if (parts.length < 2) return symbol;
  const pair = parts.slice(1).join("_");
  // Try to extract base: known quote currencies
  for (const q of ["USD", "EUR", "GBP"]) {
    if (pair.endsWith(q)) {
      const base = pair.slice(0, pair.length - q.length);
      if (base === "XBT") return "BTC";
      return base;
    }
  }
  return pair;
}

/**
 * Parse quote from perpetual symbol.
 */
function parsePerpQuote(symbol: string): string {
  const parts = symbol.split("_");
  if (parts.length < 2) return "USD";
  const pair = parts.slice(1).join("_");
  for (const q of ["USD", "EUR", "GBP"]) {
    if (pair.endsWith(q)) return q;
  }
  return "USD";
}

/**
 * Build fee schedule JSONB from Kraken taker/maker fee tiers.
 */
function buildFeeSchedule(
  fees: number[][] | undefined,
  feesMaker: number[][] | undefined,
): unknown[] | null {
  if (!fees || fees.length === 0) return null;
  return fees.map((tier, i) => ({
    volume: tier[0],
    taker: tier[1],
    maker: feesMaker?.[i]?.[1] ?? tier[1],
  }));
}

// --- Sync functions ---

export interface KrakenSyncOptions {
  spot?: boolean;
  perps?: boolean;
  noDelete?: boolean;
  dryRun?: boolean;
}

export interface KrakenSyncResult {
  spot: { fetched: number; online: number; upserted: number; deleted: number };
  perps: { fetched: number; tradeable: number; upserted: number; deleted: number };
}

/**
 * Sync Kraken spot pairs from the AssetPairs API.
 */
async function syncSpot(
  dryRun: boolean,
  noDelete: boolean,
): Promise<KrakenSyncResult["spot"]> {
  console.log("\n[Kraken Spot] Fetching asset pairs...");
  const res = await fetch(KRAKEN_SPOT_URL, {
    signal: AbortSignal.timeout(API_TIMEOUT_MS),
  });
  if (!res.ok) {
    throw new Error(`Kraken API error: ${res.status} ${await res.text()}`);
  }
  const data: KrakenSpotResponse = await res.json();
  if (data.error && data.error.length > 0) {
    throw new Error(`Kraken API errors: ${data.error.join(", ")}`);
  }

  const allPairs = Object.entries(data.result);
  const result = { fetched: allPairs.length, online: 0, upserted: 0, deleted: 0 };

  // Filter to online pairs and convert
  const pairRows: NewPair[] = [];
  for (const [pairId, pair] of allPairs) {
    if (pair.status !== "online") continue;

    const base = normalizeBase(pair.base);
    const quote = normalizeQuote(pair.quote);
    const wsName = pair.wsname ?? pair.altname ?? pairId;

    pairRows.push({
      pairId,
      exchange: "kraken",
      base,
      quote,
      wsName,
      status: "active",
      lotDecimals: pair.lot_decimals,
      pairDecimals: pair.pair_decimals,
      marketType: "spot",
      altname: pair.altname,
      tickSize: pair.tick_size ?? null,
      orderMin: pair.ordermin ?? null,
      costMin: pair.costmin ?? null,
      feeSchedule: buildFeeSchedule(pair.fees, pair.fees_maker),
    });
  }

  result.online = pairRows.length;
  console.log(`[Kraken Spot] Fetched ${result.fetched} pairs, ${result.online} online`);

  if (dryRun) {
    console.log("[Kraken Spot] Dry run — skipping upsert");
    return result;
  }

  const db = getDb();
  result.upserted = await upsertSpotPairs(db, pairRows);
  console.log(`[Kraken Spot] Upserted ${result.upserted} spot pairs`);

  if (!noDelete) {
    const currentIds = pairRows.map((p) => p.pairId);
    result.deleted = await softDeleteMissingPairs("kraken", "spot", currentIds);
    if (result.deleted > 0) {
      console.log(`[Kraken Spot] Soft-deleted ${result.deleted} missing pairs`);
    }
  }

  return result;
}

/**
 * Sync Kraken perpetual contracts from the Futures API.
 * Merges instrument metadata with live ticker data.
 */
async function syncPerps(
  dryRun: boolean,
  noDelete: boolean,
): Promise<KrakenSyncResult["perps"]> {
  console.log("\n[Kraken Perps] Fetching instruments...");

  // Fetch instruments and tickers in parallel
  const [instrumentsRes, tickersRes] = await Promise.all([
    fetch(KRAKEN_FUTURES_INSTRUMENTS_URL, { signal: AbortSignal.timeout(API_TIMEOUT_MS) }),
    fetch(KRAKEN_FUTURES_TICKERS_URL, { signal: AbortSignal.timeout(API_TIMEOUT_MS) }),
  ]);

  if (!instrumentsRes.ok) {
    throw new Error(`Kraken Futures instruments API error: ${instrumentsRes.status}`);
  }
  if (!tickersRes.ok) {
    throw new Error(`Kraken Futures tickers API error: ${tickersRes.status}`);
  }

  const instrumentsData: KrakenFuturesInstrumentsResponse = await instrumentsRes.json();
  const tickersData: KrakenFuturesTickersResponse = await tickersRes.json();

  // Index tickers by symbol for lookup
  const tickerMap = new Map<string, KrakenFuturesTicker>();
  for (const t of tickersData.tickers) {
    tickerMap.set(t.symbol, t);
  }

  const allInstruments = instrumentsData.instruments;
  const result = { fetched: allInstruments.length, tradeable: 0, upserted: 0, deleted: 0 };

  // Filter to tradeable perpetuals and convert
  const pairRows: NewPair[] = [];
  for (const inst of allInstruments) {
    if (!inst.tradeable) continue;

    const ticker = tickerMap.get(inst.symbol);
    const base = parsePerpBase(inst.symbol);
    const quote = parsePerpQuote(inst.symbol);

    pairRows.push({
      pairId: inst.symbol,
      exchange: "kraken",
      base,
      quote,
      wsName: inst.symbol,
      status: "active",
      marketType: "perpetual",
      underlying: inst.underlying ?? null,
      contractSize: inst.contractSize != null ? String(inst.contractSize) : null,
      contractType: inst.type ?? null,
      markPrice: ticker?.markPrice != null ? String(ticker.markPrice) : null,
      indexPrice: ticker?.indexPrice != null ? String(ticker.indexPrice) : null,
      fundingRate: ticker?.fundingRate != null ? String(ticker.fundingRate) : null,
      fundingRatePrediction: ticker?.fundingRatePrediction != null ? String(ticker.fundingRatePrediction) : null,
      openInterest: ticker?.openInterest != null ? String(ticker.openInterest) : null,
      maxPositionSize: inst.maxPositionSize != null ? String(inst.maxPositionSize) : null,
      marginLevels: inst.marginLevels ?? null,
      tradeable: inst.tradeable,
      suspended: inst.suspended ?? false,
      openingDate: inst.openingDate ? new Date(inst.openingDate) : null,
      feeScheduleUid: inst.feeScheduleUid ?? null,
      tags: inst.tags ?? null,
      lastPrice: ticker?.last != null ? String(ticker.last) : null,
      bid: ticker?.bid != null ? String(ticker.bid) : null,
      ask: ticker?.ask != null ? String(ticker.ask) : null,
      volume24h: ticker?.vol24h != null ? String(ticker.vol24h) : null,
    });
  }

  result.tradeable = pairRows.length;
  console.log(`[Kraken Perps] Fetched ${result.fetched} instruments, ${result.tradeable} tradeable`);

  if (dryRun) {
    console.log("[Kraken Perps] Dry run — skipping upsert");
    return result;
  }

  const db = getDb();
  result.upserted = await upsertPerpPairs(db, pairRows);
  console.log(`[Kraken Perps] Upserted ${result.upserted} perpetual contracts`);

  if (!noDelete) {
    const currentIds = pairRows.map((p) => p.pairId);
    result.deleted = await softDeleteMissingPairs("kraken", "perpetual", currentIds);
    if (result.deleted > 0) {
      console.log(`[Kraken Perps] Soft-deleted ${result.deleted} missing contracts`);
    }
  }

  return result;
}

/**
 * Run Kraken sync (spot + perpetuals)
 */
export async function runKrakenSync(
  options: KrakenSyncOptions = {},
): Promise<KrakenSyncResult> {
  const syncSpotFlag = options.spot ?? true;
  const syncPerpsFlag = options.perps ?? true;
  const noDelete = options.noDelete ?? false;
  const dryRun = options.dryRun ?? false;

  const result: KrakenSyncResult = {
    spot: { fetched: 0, online: 0, upserted: 0, deleted: 0 },
    perps: { fetched: 0, tradeable: 0, upserted: 0, deleted: 0 },
  };

  try {
    if (syncSpotFlag) {
      result.spot = await syncSpot(dryRun, noDelete);
    }
    if (syncPerpsFlag) {
      result.perps = await syncPerps(dryRun, noDelete);
    }

    console.log("\n=== Kraken Sync Summary ===");
    if (syncSpotFlag) {
      console.log(`Spot:  ${result.spot.upserted} upserted, ${result.spot.deleted} deleted`);
    }
    if (syncPerpsFlag) {
      console.log(`Perps: ${result.perps.upserted} upserted, ${result.perps.deleted} deleted`);
    }

    return result;
  } finally {
    await closeDb();
  }
}

/**
 * Handle `ssmd kraken` subcommands
 */
export async function handleKraken(
  subcommand: string,
  flags: Record<string, unknown>,
): Promise<void> {
  switch (subcommand) {
    case "sync": {
      // If neither --spot nor --perps specified, sync both
      const spotFlag = Boolean(flags.spot);
      const perpsFlag = Boolean(flags.perps);
      const syncBoth = !spotFlag && !perpsFlag;

      const options: KrakenSyncOptions = {
        spot: syncBoth || spotFlag,
        perps: syncBoth || perpsFlag,
        noDelete: Boolean(flags["no-delete"]),
        dryRun: Boolean(flags["dry-run"]),
      };

      try {
        await runKrakenSync(options);
      } catch (e) {
        console.error(`Kraken sync failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    default:
      console.log("Usage: ssmd kraken <command>");
      console.log();
      console.log("Commands:");
      console.log("  sync         Sync Kraken spot pairs + perpetual contracts");
      console.log();
      console.log("Options for sync:");
      console.log("  --spot       Only sync spot pairs");
      console.log("  --perps      Only sync perpetual contracts");
      console.log("  --no-delete  Skip soft-deleting missing pairs");
      console.log("  --dry-run    Fetch but don't write to database");
      Deno.exit(1);
  }
}
