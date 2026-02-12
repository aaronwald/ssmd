// dq.ts - Data Quality checks: compare NATS trades with exchange APIs
// Supports Kalshi, Kraken, and Polymarket exchanges
// Also includes `daily` subcommand for composite Phase 1+2 scoring

import { createKalshiClient, type KalshiTrade } from "../../lib/api/kalshi.ts";
import { fetchKrakenTrades } from "../../lib/api/kraken-public.ts";
import { getEnvContext } from "../utils/env-context.ts";
import { getRawSql, closeDb } from "../../lib/db/mod.ts";
import { connect as natsConnect, type NatsConnection } from "npm:nats";

// --- GCS Feed Configuration ---

const GCS_BUCKET = "ssmd-data";

interface GcsFeedConfig {
  /** Feed name used in scoring (matches dq_daily_scores.feed) */
  feed: string;
  /** GCS prefix (bucket subdirectory) */
  prefix: string;
  /** Stream subdirectory within prefix */
  stream: string;
  /** NATS stream name for message counts */
  natsStream: string;
}

const GCS_FEEDS: GcsFeedConfig[] = [
  { feed: "kalshi-crypto", prefix: "kalshi", stream: "crypto", natsStream: "PROD_KALSHI_CRYPTO" },
  { feed: "kraken-futures", prefix: "kraken-futures", stream: "futures", natsStream: "PROD_KRAKEN_FUTURES" },
  { feed: "polymarket", prefix: "polymarket", stream: "markets", natsStream: "PROD_POLYMARKET" },
];

// --- GCS Utility Functions ---

interface GcsFileInfo {
  path: string;
  name: string;
  sizeBytes: number;
  msgType: string;
  time: string; // HHMM
}

/**
 * List files in a GCS path with sizes using gcloud storage ls -l.
 * Returns parsed file info including size, msg_type, and time slot.
 */
async function listGcsFilesWithInfo(
  bucket: string,
  prefix: string,
  stream: string,
  date: string,
  ext: string = "parquet",
): Promise<GcsFileInfo[]> {
  const gsPath = `gs://${bucket}/${prefix}/${stream}/${date}/`;
  const cmd = new Deno.Command("gcloud", {
    args: ["storage", "ls", "-l", gsPath],
    stdout: "piped",
    stderr: "piped",
  });

  const output = await cmd.output();
  if (!output.success) {
    const err = new TextDecoder().decode(output.stderr);
    if (err.includes("CommandException") || err.includes("NOT_FOUND") || err.includes("matched no objects")) {
      return [];
    }
    // Non-fatal: log and return empty
    console.error(`  WARN: gcloud ls failed for ${gsPath}: ${err.slice(0, 200)}`);
    return [];
  }

  const text = new TextDecoder().decode(output.stdout);
  const files: GcsFileInfo[] = [];

  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("TOTAL:")) continue;

    // gcloud storage ls -l format: "  SIZE  DATE  gs://path"
    // Example: "  12345  2026-02-12T14:00:00Z  gs://ssmd-data/kalshi/crypto/2026-02-12/ticker_1400.parquet"
    const match = trimmed.match(/^\s*(\d+)\s+\S+\s+(gs:\/\/.+)$/);
    if (!match) continue;

    const fullPath = match[2];
    const sizeBytes = parseInt(match[1], 10);
    const fileName = fullPath.split("/").pop() ?? "";

    if (!fileName.endsWith(`.${ext}`)) continue;

    // Parse msg_type and time from filename: {msg_type}_{HHMM}.{ext}
    const baseName = fileName.replace(`.${ext}`, "");
    const lastUnderscore = baseName.lastIndexOf("_");
    if (lastUnderscore === -1) continue;

    const msgType = baseName.substring(0, lastUnderscore);
    const time = baseName.substring(lastUnderscore + 1);

    files.push({ path: fullPath, name: fileName, sizeBytes, msgType, time });
  }

  return files;
}

/**
 * Get the set of unique 15-minute time slots covered by files.
 * Returns a Set of HHMM strings.
 */
function getTimeSlots(files: GcsFileInfo[]): Set<string> {
  return new Set(files.map((f) => f.time));
}

/**
 * Generate all expected 15-minute time slots for a 24-hour day.
 */
function allExpectedSlots(): string[] {
  const slots: string[] = [];
  for (let h = 0; h < 24; h++) {
    for (let m = 0; m < 60; m += 15) {
      slots.push(`${String(h).padStart(2, "0")}${String(m).padStart(2, "0")}`);
    }
  }
  return slots;
}

/**
 * Find gaps (missing time slots) in the file set.
 * Returns contiguous gap ranges with start/end times and duration.
 */
function findGaps(
  files: GcsFileInfo[],
): { start: string; end: string; durationMinutes: number }[] {
  const expected = allExpectedSlots();
  const present = getTimeSlots(files);
  const gaps: { start: string; end: string; durationMinutes: number }[] = [];

  let gapStart: string | null = null;
  let gapCount = 0;

  for (const slot of expected) {
    if (!present.has(slot)) {
      if (gapStart === null) gapStart = slot;
      gapCount++;
    } else {
      if (gapStart !== null) {
        gaps.push({
          start: `${gapStart.slice(0, 2)}:${gapStart.slice(2)}`,
          end: `${slot.slice(0, 2)}:${slot.slice(2)}`,
          durationMinutes: gapCount * 15,
        });
        gapStart = null;
        gapCount = 0;
      }
    }
  }
  // Handle trailing gap
  if (gapStart !== null) {
    gaps.push({
      start: `${gapStart.slice(0, 2)}:${gapStart.slice(2)}`,
      end: "24:00",
      durationMinutes: gapCount * 15,
    });
  }

  return gaps;
}

// --- Shared types ---

interface DqFlags {
  _: (string | number)[];
  ticker?: string;
  window?: string; // e.g., "5m", "10m", "1h"
  env?: string;
  exchange?: string; // "kalshi" | "kraken" | "polymarket"
  detailed?: boolean;
  json?: boolean;
}

interface NatsTrade {
  tradeId: string;
  ticker: string;
  price: number;
  size: number;
  side: string;
  timestamp: number; // Unix seconds
}

interface ApiTrade {
  tradeId: string;
  ticker: string;
  price: number;
  size: number;
  side: string;
  timestamp: number;
}

interface DqResult {
  exchange: string;
  ticker: string;
  windowStart: Date;
  windowEnd: Date;
  natsCount: number;
  apiCount: number;
  matchedCount: number;
  missingInNats: ApiTrade[];
  extraInNats: NatsTrade[];
  natsTotalSize: number;
  apiTotalSize: number;
}

// --- ExchangeAdapter interface ---

interface ExchangeAdapter {
  name: string;
  buildNatsFilter(ticker: string): { stream: string; subject: string };
  parseNatsTrade(msg: Record<string, unknown>): NatsTrade | null;
  fetchApiTrades?(ticker: string, from: number, to: number): Promise<ApiTrade[]>;
}

// --- Kalshi adapter ---

class KalshiAdapter implements ExchangeAdapter {
  name = "kalshi";

  buildNatsFilter(ticker: string): { stream: string; subject: string } {
    const category = inferCategory(ticker);
    return {
      stream: `PROD_KALSHI_${category.toUpperCase()}`,
      subject: `prod.kalshi.${category.toLowerCase()}.json.trade.${ticker}`,
    };
  }

  parseNatsTrade(msg: Record<string, unknown>): NatsTrade | null {
    if (msg.type !== "trade" || !msg.msg) return null;
    const m = msg.msg as Record<string, unknown>;
    return {
      tradeId: String(m.trade_id),
      ticker: String(m.market_ticker),
      price: Number(m.yes_price),
      size: Number(m.count),
      side: String(m.taker_side),
      timestamp: Number(m.ts),
    };
  }

  async fetchApiTrades(ticker: string, from: number, to: number): Promise<ApiTrade[]> {
    const client = createKalshiClient();
    const trades = await client.fetchAllTrades(ticker, from, to);
    return trades.map((t: KalshiTrade) => ({
      tradeId: t.trade_id,
      ticker: t.ticker,
      price: t.yes_price,
      size: t.count,
      side: t.taker_side,
      timestamp: Math.floor(new Date(t.created_time).getTime() / 1000),
    }));
  }
}

// --- Kraken adapter ---

class KrakenAdapter implements ExchangeAdapter {
  name = "kraken";

  buildNatsFilter(ticker: string): { stream: string; subject: string } {
    // ticker comes in as the pair name (e.g., "XBT/USD" or "XBTUSD")
    return {
      stream: "PROD_KRAKEN",
      subject: `prod.kraken.json.trade.${ticker}`,
    };
  }

  parseNatsTrade(msg: Record<string, unknown>): NatsTrade | null {
    if (msg.channel !== "trade") return null;
    const dataArr = msg.data as Record<string, unknown>[] | undefined;
    if (!Array.isArray(dataArr) || dataArr.length === 0) return null;

    // Each NATS message may contain multiple trades in data[];
    // return the first one here — caller should handle batch
    const t = dataArr[0];
    return {
      tradeId: String(t.trade_id ?? `${t.timestamp}-${t.price}-${t.qty}`),
      ticker: String(t.symbol ?? msg.symbol ?? ""),
      price: Number(t.price),
      size: Number(t.qty),
      side: String(t.side),
      timestamp: Number(t.timestamp),
    };
  }

  /**
   * Parse all trades from a single NATS message (Kraken batches trades in data[])
   */
  parseAllNatsTrades(msg: Record<string, unknown>): NatsTrade[] {
    if (msg.channel !== "trade") return [];
    const dataArr = msg.data as Record<string, unknown>[] | undefined;
    if (!Array.isArray(dataArr)) return [];

    return dataArr.map((t) => ({
      tradeId: String(t.trade_id ?? `${t.timestamp}-${t.price}-${t.qty}`),
      ticker: String(t.symbol ?? msg.symbol ?? ""),
      price: Number(t.price),
      size: Number(t.qty),
      side: String(t.side),
      timestamp: Number(t.timestamp),
    }));
  }

  async fetchApiTrades(ticker: string, from: number, to: number): Promise<ApiTrade[]> {
    // Kraken /Trades since is nanoseconds
    const sinceNano = String(from * 1_000_000_000);
    const result = await fetchKrakenTrades(ticker, sinceNano);

    return result.trades
      .filter((t) => t.time >= from && t.time < to)
      .map((t) => ({
        tradeId: t.tradeId,
        ticker,
        price: parseFloat(t.price),
        size: parseFloat(t.volume),
        side: t.side,
        timestamp: t.time,
      }));
  }
}

// --- Polymarket adapter ---

class PolymarketAdapter implements ExchangeAdapter {
  name = "polymarket";

  buildNatsFilter(ticker: string): { stream: string; subject: string } {
    return {
      stream: "PROD_POLYMARKET",
      subject: `prod.polymarket.json.last_trade_price.${ticker}`,
    };
  }

  parseNatsTrade(msg: Record<string, unknown>): NatsTrade | null {
    // Polymarket trade messages have asset_id, price, side, size, timestamp
    const assetId = msg.asset_id as string | undefined;
    if (!assetId) return null;

    const price = Number(msg.price);
    const ts = Number(msg.timestamp);

    return {
      tradeId: `${assetId}-${price}-${ts}`,
      ticker: String(msg.market ?? assetId),
      price,
      size: Number(msg.size ?? 1),
      side: String(msg.side ?? "unknown"),
      timestamp: ts,
    };
  }

  // No public trade API for v1 — NATS-only checks
}

// --- Adapter factory ---

function getAdapter(exchange: string): ExchangeAdapter {
  switch (exchange) {
    case "kalshi":
      return new KalshiAdapter();
    case "kraken":
      return new KrakenAdapter();
    case "polymarket":
      return new PolymarketAdapter();
    default:
      throw new Error(`Unsupported exchange: ${exchange}. Valid: kalshi, kraken, polymarket`);
  }
}

// --- Entry point ---

export async function handleDq(subcommand: string, flags: DqFlags): Promise<void> {
  switch (subcommand) {
    case "trades":
      await runTradesDqCheck(flags);
      break;

    case "secmaster":
      await runSecmasterDqCheck(flags);
      break;

    case "daily":
      await runDailyDqCheck(flags);
      break;

    case "help":
    default:
      printDqHelp();
      break;
  }
}

// --- Trades DQ check ---

async function runTradesDqCheck(flags: DqFlags): Promise<void> {
  const ticker = flags.ticker;
  if (!ticker) {
    console.error("Error: --ticker is required");
    console.log("Usage: ssmd dq trades --ticker <TICKER> [--exchange kalshi] [--window 5m]");
    Deno.exit(1);
  }

  const exchange = flags.exchange || "kalshi";
  const adapter = getAdapter(exchange);

  // Parse window (default 5 minutes)
  const windowStr = flags.window || "5m";
  const windowMs = parseWindow(windowStr);
  if (!windowMs) {
    console.error(`Invalid window format: ${windowStr}. Use format like '5m', '10m', '1h'`);
    Deno.exit(1);
  }

  const now = Date.now();
  const windowEnd = new Date(now);
  const windowStart = new Date(now - windowMs);

  // Add 5s buffer for boundary effects
  const bufferSec = 5;
  const apiMinTs = Math.floor(windowStart.getTime() / 1000) - bufferSec;
  const apiMaxTs = Math.floor(windowEnd.getTime() / 1000) + bufferSec;

  console.log(`DQ Check: ${exchange}/${ticker}`);
  console.log(`Window: ${windowStart.toISOString()} to ${windowEnd.toISOString()}`);
  console.log("=".repeat(70));
  console.log();

  // Get context for kubectl commands
  const context = await getEnvContext(flags.env);

  // Fetch NATS trades
  console.log("Fetching trades...");
  const natsTrades = await fetchNatsTrades(adapter, ticker, windowStart, windowEnd, context.cluster);

  const windowStartSec = Math.floor(windowStart.getTime() / 1000);
  const windowEndSec = Math.floor(windowEnd.getTime() / 1000);

  if (adapter.fetchApiTrades) {
    // Full NATS vs API comparison
    console.log(`  API: fetching ${exchange} trades for ${ticker} [${apiMinTs} - ${apiMaxTs}]`);
    const apiTradesRaw = await adapter.fetchApiTrades(ticker, apiMinTs, apiMaxTs);

    // Filter API trades to exact window
    const apiTrades = apiTradesRaw.filter(
      (t) => t.timestamp >= windowStartSec && t.timestamp < windowEndSec,
    );
    console.log(`  API: Found ${apiTrades.length} trades in window`);

    const result = compareTrades(exchange, ticker, windowStart, windowEnd, natsTrades, apiTrades);
    printResults(result, flags.detailed || false);
  } else {
    // NATS-only checks (gap detection, count summary)
    console.log(`  (${exchange} has no public trade API — NATS-only checks)`);
    printNatsOnlyResults(exchange, ticker, windowStart, windowEnd, natsTrades, flags.detailed || false);
  }
}

// --- NATS fetch (shared across adapters) ---

async function fetchNatsTrades(
  adapter: ExchangeAdapter,
  ticker: string,
  windowStart: Date,
  windowEnd: Date,
  _cluster: string,
): Promise<NatsTrade[]> {
  const { stream, subject } = adapter.buildNatsFilter(ticker);

  const windowMs = windowEnd.getTime() - windowStart.getTime();
  const windowSec = Math.ceil(windowMs / 1000);
  const sinceDuration = `${windowSec}s`;

  const consumerName = `dq-${Date.now()}`;

  console.log(`  NATS: stream=${stream}, filter=${subject}, since=${sinceDuration}`);

  const cmd = new Deno.Command("kubectl", {
    args: [
      "exec",
      "-n",
      "nats",
      "deploy/nats-box",
      "--",
      "sh",
      "-c",
      `nats consumer add ${stream} ${consumerName} \
        --ephemeral \
        --deliver "${sinceDuration}" \
        --filter "${subject}" \
        --ack none \
        --pull \
        --inactive-threshold 30s \
        --defaults \
        -s nats://nats:4222 >/dev/null && \
      nats consumer next ${stream} ${consumerName} --count 10000 --raw -s nats://nats:4222`,
    ],
    stdout: "piped",
    stderr: "piped",
  });

  const { stdout } = await cmd.output();
  const output = new TextDecoder().decode(stdout);
  const trades: NatsTrade[] = [];
  const windowStartSec = Math.floor(windowStart.getTime() / 1000);
  const windowEndSec = Math.floor(windowEnd.getTime() / 1000);

  const lines = output.split("\n");
  const isKraken = adapter instanceof KrakenAdapter;

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || !trimmed.startsWith("{")) continue;

    try {
      const msg = JSON.parse(trimmed);

      if (isKraken) {
        // Kraken batches trades in data[] — expand all
        const batch = (adapter as KrakenAdapter).parseAllNatsTrades(msg);
        for (const t of batch) {
          if (t.timestamp >= windowStartSec && t.timestamp < windowEndSec) {
            trades.push(t);
          }
        }
      } else {
        const trade = adapter.parseNatsTrade(msg);
        if (trade && trade.timestamp >= windowStartSec && trade.timestamp < windowEndSec) {
          trades.push(trade);
        }
      }
    } catch {
      // Skip non-JSON lines
    }
  }

  console.log(`  NATS: Found ${trades.length} trades in window`);
  return trades;
}

// --- Compare trades (NATS vs API) ---

export function compareTrades(
  exchange: string,
  ticker: string,
  windowStart: Date,
  windowEnd: Date,
  natsTrades: NatsTrade[],
  apiTrades: ApiTrade[],
): DqResult {
  const natsById = new Map<string, NatsTrade>();
  for (const t of natsTrades) {
    natsById.set(t.tradeId, t);
  }

  const apiById = new Map<string, ApiTrade>();
  for (const t of apiTrades) {
    apiById.set(t.tradeId, t);
  }

  const matched: string[] = [];
  const missingInNats: ApiTrade[] = [];
  const extraInNats: NatsTrade[] = [];

  for (const [id, trade] of apiById) {
    if (natsById.has(id)) {
      matched.push(id);
    } else {
      missingInNats.push(trade);
    }
  }

  for (const [id, trade] of natsById) {
    if (!apiById.has(id)) {
      extraInNats.push(trade);
    }
  }

  const natsTotalSize = natsTrades.reduce((sum, t) => sum + t.size, 0);
  const apiTotalSize = apiTrades.reduce((sum, t) => sum + t.size, 0);

  return {
    exchange,
    ticker,
    windowStart,
    windowEnd,
    natsCount: natsTrades.length,
    apiCount: apiTrades.length,
    matchedCount: matched.length,
    missingInNats,
    extraInNats,
    natsTotalSize,
    apiTotalSize,
  };
}

// --- Print results (full comparison) ---

function printResults(result: DqResult, detailed: boolean): void {
  console.log();

  const matchRate =
    result.apiCount > 0
      ? ((result.matchedCount / result.apiCount) * 100).toFixed(1)
      : "N/A";

  const status =
    result.missingInNats.length === 0 && result.extraInNats.length === 0
      ? "OK"
      : result.missingInNats.length > 0
        ? "WARN (missing)"
        : "WARN (extra)";

  console.log("SUMMARY");
  console.log("-".repeat(40));
  console.log(
    `  NATS trades:     ${result.natsCount.toString().padStart(6)}    API trades:     ${result.apiCount.toString().padStart(6)}`,
  );
  console.log(
    `  NATS size:       ${result.natsTotalSize.toString().padStart(6)}    API size:       ${result.apiTotalSize.toString().padStart(6)}`,
  );
  console.log(
    `  Match rate:      ${matchRate.padStart(5)}%    Status:         ${status}`,
  );
  console.log();

  if (result.missingInNats.length > 0) {
    console.log(`MISSING IN NATS (${result.missingInNats.length} trades)`);
    console.log("-".repeat(40));
    if (detailed) {
      for (const t of result.missingInNats) {
        const ts = new Date(t.timestamp * 1000).toISOString();
        console.log(
          `  ${t.tradeId.substring(0, 12).padEnd(12)}  ${ts}  ${t.price}  ${t.size} size  ${t.side}`,
        );
      }
    } else {
      console.log("  (use --detailed to see individual trades)");
    }
    console.log();
  }

  if (result.extraInNats.length > 0) {
    console.log(`EXTRA IN NATS (${result.extraInNats.length} trades)`);
    console.log("-".repeat(40));
    if (detailed) {
      for (const t of result.extraInNats) {
        const ts = new Date(t.timestamp * 1000).toISOString();
        console.log(
          `  ${t.tradeId.substring(0, 12).padEnd(12)}  ${ts}  ${t.price}  ${t.size} size  ${t.side}`,
        );
      }
    } else {
      console.log("  (use --detailed to see individual trades)");
    }
    console.log();
  }
}

// --- Print NATS-only results (no API comparison) ---

function printNatsOnlyResults(
  exchange: string,
  ticker: string,
  windowStart: Date,
  windowEnd: Date,
  trades: NatsTrade[],
  detailed: boolean,
): void {
  console.log();
  console.log("SUMMARY (NATS-only)");
  console.log("-".repeat(40));
  console.log(`  Exchange:        ${exchange}`);
  console.log(`  Ticker:          ${ticker}`);
  console.log(`  Window:          ${windowStart.toISOString()} → ${windowEnd.toISOString()}`);
  console.log(`  Trade count:     ${trades.length}`);

  if (trades.length === 0) {
    console.log("  [WARN] No trades found in window");
    return;
  }

  const totalSize = trades.reduce((s, t) => s + t.size, 0);
  console.log(`  Total size:      ${totalSize}`);

  // Gap detection: sort by timestamp, flag gaps > 60s
  const sorted = [...trades].sort((a, b) => a.timestamp - b.timestamp);
  const gaps: { from: number; to: number; durationSec: number }[] = [];
  for (let i = 1; i < sorted.length; i++) {
    const gap = sorted[i].timestamp - sorted[i - 1].timestamp;
    if (gap > 60) {
      gaps.push({
        from: sorted[i - 1].timestamp,
        to: sorted[i].timestamp,
        durationSec: gap,
      });
    }
  }

  if (gaps.length > 0) {
    console.log(`  [WARN] ${gaps.length} gap(s) > 60s detected`);
    if (detailed) {
      for (const g of gaps) {
        const fromTs = new Date(g.from * 1000).toISOString();
        const toTs = new Date(g.to * 1000).toISOString();
        console.log(`    ${fromTs} → ${toTs} (${g.durationSec}s)`);
      }
    }
  } else {
    console.log("  [OK] No gaps > 60s");
  }
  console.log();
}

// --- Secmaster DQ check ---

async function runSecmasterDqCheck(flags: DqFlags): Promise<void> {
  const envCtx = await getEnvContext(flags.env);
  // Use the ssmd-data API URL based on environment
  // In prod, port-forward or set SSMD_API_URL; default to localhost
  const apiUrl = Deno.env.get("SSMD_API_URL") || "http://localhost:3000";
  const apiKey = Deno.env.get("SSMD_API_KEY") || "";

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (apiKey) {
    headers["X-API-Key"] = apiKey;
  }

  console.log();
  console.log("=== Secmaster DQ Check ===");
  console.log(`  Environment: ${envCtx.envName}`);
  console.log(`  API: ${apiUrl}`);
  console.log();

  // 1. Pair count and namespace check
  const pairsRes = await fetch(`${apiUrl}/v1/pairs?limit=10000`, { headers });
  if (!pairsRes.ok) {
    console.error(`Failed to fetch pairs: ${pairsRes.status}`);
    Deno.exit(1);
  }
  const { pairs } = await pairsRes.json() as { pairs: { pairId: string; status: string; updatedAt: string }[] };

  const malformed = pairs.filter((p) => !p.pairId.includes(":"));
  if (malformed.length > 0) {
    console.log(`[WARN] ${malformed.length} pairs missing namespace prefix`);
    for (const p of malformed.slice(0, 5)) {
      console.log(`  - ${p.pairId}`);
    }
    if (malformed.length > 5) console.log(`  ... and ${malformed.length - 5} more`);
  } else {
    console.log(`[OK] All ${pairs.length} pairs have namespace prefix`);
  }

  // 2. Stale data (updated > 24h ago)
  const now = Date.now();
  const stale = pairs.filter(
    (p) => p.status === "active" && now - new Date(p.updatedAt).getTime() > 86400000,
  );
  if (stale.length > 0) {
    console.log(`[WARN] ${stale.length} active pairs not updated in >24h`);
    for (const p of stale.slice(0, 5)) {
      console.log(`  - ${p.pairId} (updated: ${p.updatedAt})`);
    }
    if (stale.length > 5) console.log(`  ... and ${stale.length - 5} more`);
  } else {
    console.log(`[OK] All active pairs updated within 24h`);
  }

  // 3. Stats check
  const statsRes = await fetch(`${apiUrl}/v1/secmaster/stats`, { headers });
  if (!statsRes.ok) {
    console.error(`Failed to fetch stats: ${statsRes.status}`);
    Deno.exit(1);
  }
  const stats = await statsRes.json();
  console.log();
  console.log("Secmaster Stats:");
  if (stats.events) console.log(`  Events:     ${JSON.stringify(stats.events)}`);
  if (stats.markets) console.log(`  Markets:    ${JSON.stringify(stats.markets)}`);
  if (stats.pairs) console.log(`  Pairs:      ${JSON.stringify(stats.pairs)}`);
  if (stats.conditions) console.log(`  Conditions: ${JSON.stringify(stats.conditions)}`);
  console.log();
}

// --- Daily DQ check ---

interface DailyReport {
  date: string;
  feeds: Record<string, FeedScore>;
  composite: number;
  grade: "GREEN" | "YELLOW" | "RED";
  issues: string[];
  prometheusDegraded: boolean;
}

interface FeedScore {
  score: number;
  [key: string]: unknown;
}

function getPrometheusUrl(): string {
  return Deno.env.get("PROMETHEUS_URL") ??
    "http://kube-prometheus-stack-prometheus.observability.svc:9090";
}

function getNatsUrlDq(): string {
  return Deno.env.get("NATS_URL") ?? "nats://nats.nats.svc:4222";
}

async function promQuery(promUrl: string, query: string): Promise<number | null> {
  try {
    const url = `${promUrl}/api/v1/query?query=${encodeURIComponent(query)}`;
    const res = await fetch(url, { signal: AbortSignal.timeout(10000) });
    if (!res.ok) return null;
    const json = await res.json();
    const results = json?.data?.result;
    if (!Array.isArray(results) || results.length === 0) return null;
    return parseFloat(results[0].value[1]);
  } catch {
    return null;
  }
}

function linearScale(value: number, max: number): number {
  if (value <= 0) return 0;
  if (value >= max) return 100;
  return Math.round((value / max) * 100);
}

function idleScore(seconds: number | null, tiers: number[][]): number {
  if (seconds == null) return 0;
  for (const [threshold, score] of tiers) {
    if (seconds < threshold) return score;
  }
  return 0;
}

async function scoreConnectorFeed(
  feed: string,
  category: string | undefined,
  msgMax: number,
  mktMax: number | null,
  idleTiers: number[][],
  streamName: string,
  nc: NatsConnection,
  promUrl: string,
  promAvailable: boolean,
): Promise<{ score: number; details: Record<string, unknown> }> {
  const labelFilter = category
    ? `feed="${feed}",category="${category}"`
    : `feed="${feed}"`;

  let wsConnected: number | null = null;
  let messageCount: number | null = null;
  let idleSec: number | null = null;
  let marketsSubscribed: number | null = null;

  if (promAvailable) {
    [wsConnected, messageCount, idleSec, marketsSubscribed] = await Promise.all([
      promQuery(promUrl, `ssmd_connector_websocket_connected{${labelFilter}}`),
      promQuery(promUrl, `increase(ssmd_connector_messages_total{${labelFilter}}[24h])`),
      promQuery(promUrl, `ssmd_connector_idle_seconds{${labelFilter}}`),
      mktMax != null
        ? promQuery(promUrl, `ssmd_connector_markets_subscribed{${labelFilter}}`)
        : Promise.resolve(null),
    ]);
  }

  // NATS stream check
  let streamHasData = false;
  try {
    const jsm = await nc.jetstreamManager();
    const info = await jsm.streams.info(streamName);
    streamHasData = info.state.messages > 0;
  } catch {
    // stream may not exist
  }

  const checks: { name: string; weight: number; value: number }[] = [];
  const cap = promAvailable ? 100 : 50;

  checks.push({
    name: "ws_connected",
    weight: mktMax != null ? 0.30 : 0.35,
    value: Math.min(wsConnected === 1 ? 100 : 0, cap),
  });
  checks.push({
    name: "message_flow",
    weight: mktMax != null ? 0.25 : 0.30,
    value: Math.min(messageCount != null ? linearScale(messageCount, msgMax) : 0, cap),
  });
  checks.push({
    name: "idle_time",
    weight: 0.20,
    value: Math.min(idleSec != null ? idleScore(idleSec, idleTiers) : 0, cap),
  });
  if (mktMax != null) {
    checks.push({
      name: "markets_subscribed",
      weight: 0.15,
      value: Math.min(marketsSubscribed != null ? linearScale(marketsSubscribed, mktMax) : 0, cap),
    });
  }
  checks.push({
    name: "stream_has_data",
    weight: mktMax != null ? 0.10 : 0.15,
    value: streamHasData ? 100 : 0,
  });

  const score = Math.round(checks.reduce((s, c) => s + c.value * c.weight, 0));

  return {
    score,
    details: {
      wsConnected,
      messages: messageCount != null ? Math.round(messageCount) : null,
      idleSec: idleSec != null ? Math.round(idleSec) : null,
      markets: marketsSubscribed != null ? Math.round(marketsSubscribed) : null,
      streamHasData,
    },
  };
}

async function scoreFundingRate(
  sql: ReturnType<typeof getRawSql>,
  promUrl: string,
  promAvailable: boolean,
): Promise<{ score: number; details: Record<string, unknown> }> {
  const cap = promAvailable ? 100 : 50;

  // Consumer connected (Prometheus)
  let consumerConnected: number | null = null;
  let flushCount: number | null = null;
  if (promAvailable) {
    [consumerConnected, flushCount] = await Promise.all([
      promQuery(promUrl, "ssmd_funding_rate_connected"),
      promQuery(promUrl, "increase(ssmd_funding_rate_flushes_total[24h])"),
    ]);
  }

  // PostgreSQL queries
  type Row = { value: string | number | null };
  const [maxSnapshotRow, countRow, productsRow] = await Promise.all([
    sql`SELECT MAX(snapshot_at) as value FROM pair_snapshots` as Promise<Row[]>,
    sql`SELECT COUNT(*)::int as value FROM pair_snapshots WHERE snapshot_at > NOW() - INTERVAL '24 hours'` as Promise<Row[]>,
    sql`SELECT COUNT(DISTINCT pair_id)::int as value FROM pair_snapshots WHERE snapshot_at > NOW() - INTERVAL '1 hour'` as Promise<Row[]>,
  ]);

  const maxSnapshot = maxSnapshotRow[0]?.value
    ? new Date(String(maxSnapshotRow[0].value))
    : null;
  const snapshotCount = Number(countRow[0]?.value ?? 0);
  const productCount = Number(productsRow[0]?.value ?? 0);

  // Snapshot recency score
  let recencyScore = 0;
  if (maxSnapshot) {
    const ageMins = (Date.now() - maxSnapshot.getTime()) / 60000;
    if (ageMins < 10) recencyScore = 100;
    else if (ageMins < 30) recencyScore = 75;
    else if (ageMins < 60) recencyScore = 25;
  }

  // Daily snapshot count score (540 = 5min intervals * 24h * ~2 products with headroom)
  let countScore = 0;
  if (snapshotCount >= 540) countScore = 100;
  else if (snapshotCount >= 200) countScore = 50;
  else if (snapshotCount > 0) countScore = linearScale(snapshotCount, 200);

  // Products score
  const productsScore = productCount >= 2 ? 100 : productCount === 1 ? 50 : 0;

  // Flush rate score
  const flushScore = flushCount != null ? Math.min(linearScale(flushCount, 200), cap) : 0;

  const checks = [
    { name: "consumer_connected", weight: 0.25, value: Math.min(consumerConnected === 1 ? 100 : 0, cap) },
    { name: "snapshot_recency", weight: 0.25, value: Math.min(recencyScore, cap) },
    { name: "daily_snapshot_count", weight: 0.25, value: Math.min(countScore, cap) },
    { name: "both_products", weight: 0.15, value: Math.min(productsScore, cap) },
    { name: "flush_rate", weight: 0.10, value: Math.min(flushScore, cap) },
  ];

  const score = Math.round(checks.reduce((s, c) => s + c.value * c.weight, 0));
  const lastFlushAge = maxSnapshot
    ? Math.round((Date.now() - maxSnapshot.getTime()) / 1000)
    : null;

  return {
    score,
    details: {
      snapshots: snapshotCount,
      products: productCount,
      lastFlushAge,
      consumerConnected,
      flushCount: flushCount != null ? Math.round(flushCount) : null,
    },
  };
}

const ARCHIVERS = ["kalshi-archiver", "kraken-futures-archiver", "polymarket-archiver"];

async function scoreArchiveSync(
  sql: ReturnType<typeof getRawSql>,
): Promise<{ score: number; details: Record<string, unknown> }> {
  const archiverScores: Record<string, { score: number; lastSyncAge: number | null }> = {};

  for (const name of ARCHIVERS) {
    const rows = await sql`
      SELECT synced_at FROM archiver_sync_log
      WHERE archiver_name = ${name} AND success = true
      ORDER BY synced_at DESC LIMIT 1
    `;

    if (rows.length === 0) {
      archiverScores[name] = { score: 0, lastSyncAge: null };
      continue;
    }

    const ageHours = (Date.now() - new Date(rows[0].synced_at).getTime()) / (1000 * 60 * 60);
    let score: number;
    if (ageHours < 5) score = 100;
    else if (ageHours < 9) score = 75;
    else if (ageHours < 13) score = 50;
    else if (ageHours < 24) score = 25;
    else score = 0;

    archiverScores[name] = { score, lastSyncAge: Math.round(ageHours * 10) / 10 };
  }

  const scores = Object.values(archiverScores);
  const avgScore = Math.round(scores.reduce((sum, s) => sum + s.score, 0) / scores.length);

  return {
    score: avgScore,
    details: archiverScores,
  };
}

// --- Completeness scoring ---

async function scoreCompleteness(
  feedConfig: GcsFeedConfig,
  date: string,
  sql: ReturnType<typeof getRawSql>,
  nc: NatsConnection,
): Promise<{ score: number; details: Record<string, unknown> }> {
  // 1. Get actual message count from NATS stream (last 24h)
  let actualMessages = 0;
  try {
    const jsm = await nc.jetstreamManager();
    const info = await jsm.streams.info(feedConfig.natsStream);
    actualMessages = info.state.messages;
  } catch {
    // Stream may not exist
  }

  // 2. Get historical baseline (avg of last 7 days) from dq_daily_scores
  let expectedMessages = 0;
  try {
    type AvgRow = { avg_msgs: string | null };
    const rows = await sql`
      SELECT ROUND(AVG(actual_messages))::int as avg_msgs
      FROM dq_daily_scores
      WHERE feed = ${feedConfig.feed}
        AND check_date >= (${date}::date - INTERVAL '7 days')
        AND check_date < ${date}::date
        AND actual_messages IS NOT NULL
        AND actual_messages > 0
    ` as AvgRow[];
    expectedMessages = rows[0]?.avg_msgs ? parseInt(rows[0].avg_msgs) : 0;
  } catch {
    // Table may not have history yet
  }

  // 3. List GCS files for gap detection
  const files = await listGcsFilesWithInfo(GCS_BUCKET, feedConfig.prefix, feedConfig.stream, date);
  const gaps = findGaps(files);
  const totalGapMinutes = gaps.reduce((sum, g) => sum + g.durationMinutes, 0);

  // 4. Calculate file coverage (% of 96 expected 15-min slots)
  const expectedSlots = 96; // 24h × 4 per hour
  const presentSlots = getTimeSlots(files).size;
  const coveragePct = expectedSlots > 0 ? Math.round((presentSlots / expectedSlots) * 10000) / 100 : 0;

  // 5. Score components
  // Message ratio: actual vs expected (if we have history)
  let messageScore: number;
  if (expectedMessages > 0) {
    const ratio = actualMessages / expectedMessages;
    if (ratio >= 0.9) messageScore = 100;
    else if (ratio >= 0.7) messageScore = 75;
    else if (ratio >= 0.5) messageScore = 50;
    else if (ratio > 0) messageScore = 25;
    else messageScore = 0;
  } else {
    // No history — score based on whether messages exist at all
    messageScore = actualMessages > 0 ? 75 : 0;
  }

  // Gap penalty: more gaps = lower score
  const gapScore = gaps.length === 0 ? 100
    : gaps.length <= 2 ? 75
    : gaps.length <= 5 ? 50
    : gaps.length <= 10 ? 25
    : 0;

  // Coverage score: direct percentage mapping
  const coverageScore = Math.round(coveragePct);

  const checks = [
    { name: "message_ratio", weight: 0.35, value: messageScore },
    { name: "gap_penalty", weight: 0.35, value: gapScore },
    { name: "file_coverage", weight: 0.30, value: coverageScore },
  ];

  const score = Math.round(checks.reduce((s, c) => s + c.value * c.weight, 0));

  return {
    score,
    details: {
      actualMessages,
      expectedMessages: expectedMessages || null,
      fileCount: files.length,
      presentSlots,
      expectedSlots,
      coveragePct,
      gapCount: gaps.length,
      gapTotalMinutes: totalGapMinutes,
      gaps: gaps.slice(0, 10), // Cap at 10 for JSONB storage
    },
  };
}

// --- Parquet quality scoring ---

async function scoreParquetQuality(
  feedConfig: GcsFeedConfig,
  date: string,
  sql: ReturnType<typeof getRawSql>,
): Promise<{ score: number; details: Record<string, unknown> }> {
  const files = await listGcsFilesWithInfo(GCS_BUCKET, feedConfig.prefix, feedConfig.stream, date);

  if (files.length === 0) {
    return {
      score: 0,
      details: { error: "no parquet files found", fileCount: 0 },
    };
  }

  // Analyze file health
  const zeroByteFiles = files.filter((f) => f.sizeBytes === 0);
  const msgTypes = new Set(files.map((f) => f.msgType));
  const totalBytes = files.reduce((sum, f) => sum + f.sizeBytes, 0);
  const avgBytes = Math.round(totalBytes / files.length);

  // File size consistency: check for outliers (files < 10% of average)
  const tinyFiles = avgBytes > 0
    ? files.filter((f) => f.sizeBytes > 0 && f.sizeBytes < avgBytes * 0.1)
    : [];

  // Persist per-file stats to dq_parquet_stats
  let persistedCount = 0;
  try {
    for (const file of files) {
      await sql`
        INSERT INTO dq_parquet_stats (path, feed, msg_type, date, rows, file_size_bytes, schema_valid)
        VALUES (
          ${file.path},
          ${feedConfig.feed},
          ${file.msgType},
          ${date}::date,
          0,
          ${file.sizeBytes},
          true
        )
        ON CONFLICT (path) DO UPDATE SET
          file_size_bytes = EXCLUDED.file_size_bytes,
          created_at = NOW()
      `;
      persistedCount++;
    }
  } catch (e) {
    console.error(`  WARN: Failed to persist parquet stats: ${e}`);
  }

  // Score components
  // Files present: are all expected msg_types represented?
  // We expect at least ticker and trade for kalshi/kraken, book and last_trade_price for polymarket
  const expectedMsgTypes = getExpectedMsgTypes(feedConfig.feed);
  const missingTypes = expectedMsgTypes.filter((t) => !msgTypes.has(t));
  const typeScore = expectedMsgTypes.length > 0
    ? Math.round(((expectedMsgTypes.length - missingTypes.length) / expectedMsgTypes.length) * 100)
    : (msgTypes.size > 0 ? 100 : 0);

  // File size health: penalize zero-byte and tiny files
  const healthyFiles = files.length - zeroByteFiles.length;
  const healthScore = files.length > 0 ? Math.round((healthyFiles / files.length) * 100) : 0;

  // Consistency: penalize large variance in file sizes
  const consistencyScore = tinyFiles.length === 0 ? 100
    : tinyFiles.length <= 3 ? 75
    : tinyFiles.length <= 10 ? 50
    : 25;

  const checks = [
    { name: "msg_types_present", weight: 0.40, value: typeScore },
    { name: "file_size_health", weight: 0.30, value: healthScore },
    { name: "consistency", weight: 0.30, value: consistencyScore },
  ];

  const score = Math.round(checks.reduce((s, c) => s + c.value * c.weight, 0));

  return {
    score,
    details: {
      fileCount: files.length,
      totalBytes,
      avgBytes,
      msgTypes: [...msgTypes],
      missingMsgTypes: missingTypes,
      zeroByteCount: zeroByteFiles.length,
      tinyFileCount: tinyFiles.length,
      persistedCount,
    },
  };
}

/**
 * Expected parquet message types per feed.
 */
function getExpectedMsgTypes(feed: string): string[] {
  switch (feed) {
    case "kalshi-crypto":
      return ["ticker", "trade"];
    case "kraken-futures":
      return ["ticker", "trade"];
    case "polymarket":
      return ["book", "last_trade_price"];
    default:
      return [];
  }
}

// --- SLA scoring ---

async function scoreSLA(
  feedConfig: GcsFeedConfig,
  date: string,
): Promise<{ score: number; details: Record<string, unknown> }> {
  const files = await listGcsFilesWithInfo(GCS_BUCKET, feedConfig.prefix, feedConfig.stream, date);

  if (files.length === 0) {
    return {
      score: 0,
      details: { uptimePct: 0, freshnessMins: null, hoursWithData: 0 },
    };
  }

  // 1. Uptime: % of hours with at least one file
  const hoursWithData = new Set(files.map((f) => f.time.slice(0, 2))).size;
  const uptimePct = Math.round((hoursWithData / 24) * 10000) / 100;

  // 2. Freshness: age of most recent file
  // Sort files by time slot descending to find latest
  const sorted = [...files].sort((a, b) => b.time.localeCompare(a.time));
  const latestTime = sorted[0].time;
  const latestHour = parseInt(latestTime.slice(0, 2), 10);
  const latestMin = parseInt(latestTime.slice(2), 10);

  // Calculate freshness relative to end of day (or current time if today)
  const now = new Date();
  const isToday = date === now.toISOString().slice(0, 10);
  let freshnessMins: number;

  if (isToday) {
    const currentMins = now.getUTCHours() * 60 + now.getUTCMinutes();
    const latestMins = latestHour * 60 + latestMin;
    freshnessMins = Math.max(0, currentMins - latestMins);
  } else {
    // For past dates, freshness is from end of day
    freshnessMins = (23 * 60 + 45) - (latestHour * 60 + latestMin);
  }

  // Uptime score
  const uptimeScore = Math.round(uptimePct);

  // Freshness score
  let freshnessScore: number;
  if (freshnessMins < 60) freshnessScore = 100;
  else if (freshnessMins < 180) freshnessScore = 75;
  else if (freshnessMins < 360) freshnessScore = 50;
  else if (freshnessMins < 720) freshnessScore = 25;
  else freshnessScore = 0;

  const checks = [
    { name: "uptime", weight: 0.60, value: uptimeScore },
    { name: "freshness", weight: 0.40, value: freshnessScore },
  ];

  const score = Math.round(checks.reduce((s, c) => s + c.value * c.weight, 0));

  return {
    score,
    details: {
      hoursWithData,
      uptimePct,
      freshnessMins,
      latestFile: sorted[0].name,
      freshness_minutes: freshnessMins,
    },
  };
}

async function runDailyDqCheck(flags: DqFlags): Promise<void> {
  const jsonOutput = flags.json === true;
  const today = new Date().toISOString().slice(0, 10);
  const promUrl = getPrometheusUrl();
  const natsUrl = getNatsUrlDq();

  // Check Prometheus availability
  let promAvailable = true;
  try {
    const res = await fetch(`${promUrl}/api/v1/status/config`, { signal: AbortSignal.timeout(10000) });
    if (!res.ok) promAvailable = false;
  } catch {
    promAvailable = false;
  }

  if (!promAvailable && !jsonOutput) {
    console.warn("WARN: Prometheus unreachable, scores capped at 50");
  }

  // Connect to NATS
  let nc: NatsConnection;
  try {
    nc = await natsConnect({ servers: natsUrl });
  } catch (e) {
    console.error(`Failed to connect to NATS: ${e}`);
    Deno.exit(1);
  }

  // Get raw SQL for funding rate queries
  const sql = getRawSql();

  try {
    const standardIdleTiers = [[60, 100], [120, 75], [300, 25]];
    const polymarketIdleTiers = [[120, 100], [300, 75], [600, 25]];

    // Phase 1: Score connector feeds in parallel
    const [kalshi, kraken, polymarket, funding, archive] = await Promise.all([
      scoreConnectorFeed("kalshi", "crypto", 10000, 50, standardIdleTiers, "PROD_KALSHI_CRYPTO", nc, promUrl, promAvailable),
      scoreConnectorFeed("kraken-futures", undefined, 1000, 2, standardIdleTiers, "PROD_KRAKEN_FUTURES", nc, promUrl, promAvailable),
      scoreConnectorFeed("polymarket", undefined, 500, null, polymarketIdleTiers, "PROD_POLYMARKET", nc, promUrl, promAvailable),
      scoreFundingRate(sql, promUrl, promAvailable),
      scoreArchiveSync(sql),
    ]);

    // Phase 2: Score completeness, parquet quality, and SLA per GCS feed
    // These use gcloud CLI so run sequentially per feed to avoid rate limits,
    // but all three checks per feed run in parallel.
    const completenessScores: Record<string, { score: number; details: Record<string, unknown> }> = {};
    const parquetScores: Record<string, { score: number; details: Record<string, unknown> }> = {};
    const slaScores: Record<string, { score: number; details: Record<string, unknown> }> = {};

    // Check if gcloud is available before running GCS checks
    let gcloudAvailable = true;
    try {
      const check = new Deno.Command("gcloud", { args: ["--version"], stdout: "piped", stderr: "piped" });
      const checkOut = await check.output();
      if (!checkOut.success) gcloudAvailable = false;
    } catch {
      gcloudAvailable = false;
    }

    if (gcloudAvailable) {
      for (const feedConfig of GCS_FEEDS) {
        if (!jsonOutput) console.log(`  Scanning GCS: ${feedConfig.prefix}/${feedConfig.stream}...`);
        const [comp, pq, sla] = await Promise.all([
          scoreCompleteness(feedConfig, today, sql, nc),
          scoreParquetQuality(feedConfig, today, sql),
          scoreSLA(feedConfig, today),
        ]);
        completenessScores[feedConfig.feed] = comp;
        parquetScores[feedConfig.feed] = pq;
        slaScores[feedConfig.feed] = sla;
      }
    } else if (!jsonOutput) {
      console.warn("WARN: gcloud not available, skipping completeness/parquet/SLA checks");
    }

    // Aggregate Phase 2 scores (average across feeds)
    const avgScore = (scores: Record<string, { score: number }>) => {
      const vals = Object.values(scores);
      return vals.length > 0 ? Math.round(vals.reduce((s, v) => s + v.score, 0) / vals.length) : 0;
    };
    const completenessAvg = avgScore(completenessScores);
    const parquetAvg = avgScore(parquetScores);
    const slaAvg = avgScore(slaScores);

    // Composite score with Phase 2 dimensions
    // Phase 1 weights reduced to make room for Phase 2
    const hasPhase2 = gcloudAvailable && Object.keys(completenessScores).length > 0;
    let composite: number;
    if (hasPhase2) {
      composite = Math.round(
        kalshi.score * 0.20 +
        kraken.score * 0.15 +
        polymarket.score * 0.10 +
        funding.score * 0.15 +
        archive.score * 0.05 +
        completenessAvg * 0.15 +
        parquetAvg * 0.10 +
        slaAvg * 0.10
      );
    } else {
      // Fallback to Phase 1 weights when gcloud not available
      composite = Math.round(
        kalshi.score * 0.30 +
        kraken.score * 0.25 +
        polymarket.score * 0.15 +
        funding.score * 0.20 +
        archive.score * 0.10
      );
    }

    // Check hard RED overrides
    const issues: string[] = [];
    if (kalshi.details.wsConnected === 0) issues.push("Kalshi WS disconnected");
    if (kraken.details.wsConnected === 0) issues.push("Kraken WS disconnected");
    if (kalshi.details.messages === 0 || kalshi.details.messages === null) {
      issues.push("Kalshi zero messages in 24h" + (kalshi.details.messages === null ? " (Prometheus down)" : ""));
    }
    if (kraken.details.messages === 0 || kraken.details.messages === null) {
      issues.push("Kraken zero messages in 24h" + (kraken.details.messages === null ? " (Prometheus down)" : ""));
    }
    if (funding.details.lastFlushAge != null && (funding.details.lastFlushAge as number) > 3600) {
      issues.push("Funding rate snapshot older than 1h");
    }
    if (archive.score === 0) issues.push("No archiver has synced to GCS in 24+ hours");

    // Phase 2 RED overrides
    if (hasPhase2) {
      if (completenessAvg < 25) issues.push("Completeness critically low across feeds");
      if (parquetAvg === 0) issues.push("No valid parquet files found in GCS");
      if (slaAvg < 25) issues.push("SLA critically low: poor uptime/freshness");
    }

    let grade: "GREEN" | "YELLOW" | "RED";
    if (issues.length > 0 || composite < 60) {
      grade = "RED";
    } else if (composite < 85) {
      grade = "YELLOW";
    } else {
      grade = "GREEN";
    }

    const report: DailyReport = {
      date: today,
      feeds: {
        "kalshi-crypto": { score: kalshi.score, ...kalshi.details },
        "kraken-futures": { score: kraken.score, ...kraken.details },
        "polymarket": { score: polymarket.score, ...polymarket.details },
        "funding-rate": { score: funding.score, ...funding.details },
        "archive-sync": { score: archive.score, ...archive.details },
        ...(hasPhase2 ? {
          "completeness": { score: completenessAvg, ...Object.fromEntries(
            Object.entries(completenessScores).map(([k, v]) => [k, v.details])
          )},
          "parquet-quality": { score: parquetAvg, ...Object.fromEntries(
            Object.entries(parquetScores).map(([k, v]) => [k, v.details])
          )},
          "sla": { score: slaAvg, ...Object.fromEntries(
            Object.entries(slaScores).map(([k, v]) => [k, v.details])
          )},
        } : {}),
      },
      composite,
      grade,
      issues,
      prometheusDegraded: !promAvailable,
    };

    // Persist scores
    try {
      // Phase 1 feed entries
      const feedEntries: {
        feed: string;
        score: number;
        details: Record<string, unknown>;
        gapCount?: number;
        gapTotalMinutes?: number;
        coveragePct?: number;
        expectedMessages?: number;
        actualMessages?: number;
      }[] = [
        { feed: "kalshi-crypto", score: kalshi.score, details: kalshi.details },
        { feed: "kraken-futures", score: kraken.score, details: kraken.details },
        { feed: "polymarket", score: polymarket.score, details: polymarket.details },
        { feed: "funding-rate", score: funding.score, details: funding.details },
        { feed: "archive-sync", score: archive.score, details: archive.details },
      ];

      // Merge Phase 2 data into feed entries (completeness details go into the feed's row)
      if (hasPhase2) {
        for (const entry of feedEntries) {
          const comp = completenessScores[entry.feed];
          if (comp) {
            entry.details = {
              ...entry.details,
              completeness: comp.details,
              parquetQuality: parquetScores[entry.feed]?.details,
              sla: slaScores[entry.feed]?.details,
            };
            entry.gapCount = comp.details.gapCount as number;
            entry.gapTotalMinutes = comp.details.gapTotalMinutes as number;
            entry.coveragePct = comp.details.coveragePct as number;
            entry.actualMessages = comp.details.actualMessages as number;
            entry.expectedMessages = comp.details.expectedMessages as number | undefined;
          }
        }

        // Also persist aggregate Phase 2 scores
        feedEntries.push(
          { feed: "completeness", score: completenessAvg, details: completenessScores },
          { feed: "parquet-quality", score: parquetAvg, details: parquetScores },
          { feed: "sla", score: slaAvg, details: slaScores },
        );
      }

      for (const entry of feedEntries) {
        await sql`
          INSERT INTO dq_daily_scores (
            check_date, feed, score, composite_score, details,
            gap_count, gap_total_minutes, coverage_pct, expected_messages, actual_messages
          )
          VALUES (
            ${today}::date, ${entry.feed}, ${entry.score}, ${composite},
            ${JSON.stringify(entry.details)}::jsonb,
            ${entry.gapCount ?? null}, ${entry.gapTotalMinutes ?? null},
            ${entry.coveragePct ?? null}, ${entry.expectedMessages ?? null},
            ${entry.actualMessages ?? null}
          )
          ON CONFLICT (check_date, feed) DO UPDATE SET
            score = EXCLUDED.score,
            composite_score = EXCLUDED.composite_score,
            details = EXCLUDED.details,
            gap_count = EXCLUDED.gap_count,
            gap_total_minutes = EXCLUDED.gap_total_minutes,
            coverage_pct = EXCLUDED.coverage_pct,
            expected_messages = EXCLUDED.expected_messages,
            actual_messages = EXCLUDED.actual_messages,
            updated_at = NOW()
        `;
      }
    } catch (e) {
      // Log to stderr so JSON stdout is not corrupted
      console.error(`WARN: Failed to persist scores: ${e}`);
    }

    if (jsonOutput) {
      console.log(JSON.stringify(report));
    } else {
      console.log();
      console.log(`DQ Report: ${grade} (${composite}/100) — ${today}`);
      console.log("=".repeat(60));

      // Phase 1: Connector health
      console.log("  Connector Health:");
      const k = kalshi.details;
      const kr = kraken.details;
      const p = polymarket.details;
      const f = funding.details;
      console.log(`    Kalshi Crypto:    ${String(kalshi.score).padStart(3)}/100 (${fmtNum(k.messages as number)} msgs, ${k.markets ?? "?"} mkts, ${k.idleSec ?? "?"}s idle)`);
      console.log(`    Kraken Futures:   ${String(kraken.score).padStart(3)}/100 (${fmtNum(kr.messages as number)} msgs, ${kr.markets ?? "?"} mkts, ${kr.idleSec ?? "?"}s idle)`);
      console.log(`    Polymarket:       ${String(polymarket.score).padStart(3)}/100 (${fmtNum(p.messages as number)} msgs, ${p.idleSec ?? "?"}s idle)`);
      console.log(`    Funding Rate:     ${String(funding.score).padStart(3)}/100 (${f.snapshots ?? 0} snaps, ${f.products ?? 0} products, ${fmtAge(f.lastFlushAge as number | null)})`);
      const archDetails = archive.details as Record<string, { score: number; lastSyncAge: number | null }>;
      const archParts = Object.entries(archDetails)
        .map(([n, d]) => `${n.replace("-archiver", "")}: ${d.lastSyncAge != null ? d.lastSyncAge + "h" : "never"}`)
        .join(", ");
      console.log(`    GCS Archive:      ${String(archive.score).padStart(3)}/100 (${archParts})`);

      // Phase 2: Data quality
      if (hasPhase2) {
        console.log();
        console.log("  Data Quality (GCS):");
        console.log(`    Completeness:     ${String(completenessAvg).padStart(3)}/100`);
        for (const [feed, s] of Object.entries(completenessScores)) {
          const d = s.details;
          console.log(`      ${feed}: ${s.score}/100 (${d.fileCount} files, ${d.gapCount} gaps, ${d.coveragePct}% coverage)`);
        }
        console.log(`    Parquet Quality:  ${String(parquetAvg).padStart(3)}/100`);
        for (const [feed, s] of Object.entries(parquetScores)) {
          const d = s.details;
          console.log(`      ${feed}: ${s.score}/100 (${d.fileCount} files, ${fmtBytes(d.totalBytes as number)})`);
        }
        console.log(`    SLA:              ${String(slaAvg).padStart(3)}/100`);
        for (const [feed, s] of Object.entries(slaScores)) {
          const d = s.details;
          console.log(`      ${feed}: ${s.score}/100 (${d.hoursWithData}/24h uptime, ${d.freshnessMins ?? "?"}m fresh)`);
        }
      }

      console.log();
      console.log(`  Composite: ${composite}/100 ${grade}`);
      if (issues.length > 0) {
        console.log();
        console.log("  Issues:");
        for (const issue of issues) {
          console.log(`    - ${issue}`);
        }
      }
      if (!promAvailable) {
        console.log();
        console.log("  (Prometheus degraded — scores capped at 50)");
      }
      if (!gcloudAvailable) {
        console.log();
        console.log("  (gcloud unavailable — Phase 2 metrics skipped)");
      }
    }
  } finally {
    await nc.close();
    await closeDb();
  }
}

function fmtNum(n: number | null): string {
  if (n == null) return "?";
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
  return `${n}`;
}

function fmtAge(seconds: number | null): string {
  if (seconds == null) return "?";
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`;
  return `${(seconds / 3600).toFixed(1)}h ago`;
}

function fmtBytes(bytes: number | null): string {
  if (bytes == null) return "?";
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)}GB`;
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)}MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${bytes}B`;
}

// --- Utility functions (exported for testing) ---

export function inferCategory(ticker: string): string {
  if (ticker.startsWith("KXBTC") || ticker.startsWith("KXETH")) {
    return "crypto";
  } else if (
    ticker.startsWith("KXNBA") ||
    ticker.startsWith("KXNFL") ||
    ticker.startsWith("KXMLB")
  ) {
    return "sports";
  } else if (
    ticker.startsWith("INX") ||
    ticker.startsWith("FED") ||
    ticker.startsWith("CPI")
  ) {
    return "economics";
  } else if (
    ticker.startsWith("PRES") ||
    ticker.startsWith("SEN") ||
    ticker.startsWith("GOV")
  ) {
    return "politics";
  }
  return "crypto";
}

export function parseWindow(windowStr: string): number | null {
  const match = windowStr.match(/^(\d+)(m|h|s)$/);
  if (!match) return null;

  const value = parseInt(match[1], 10);
  const unit = match[2];

  switch (unit) {
    case "s":
      return value * 1000;
    case "m":
      return value * 60 * 1000;
    case "h":
      return value * 60 * 60 * 1000;
    default:
      return null;
  }
}

export function printDqHelp(): void {
  console.log("Usage: ssmd dq <command> [options]");
  console.log();
  console.log("Data quality checks for market data pipeline");
  console.log();
  console.log("COMMANDS:");
  console.log("  daily           Composite DQ scoring: connector health + completeness + parquet quality + SLA");
  console.log("  trades          Compare NATS trades with exchange API");
  console.log("  secmaster       Run secmaster data quality checks");
  console.log();
  console.log("OPTIONS (daily):");
  console.log("  --json              Output structured JSON to stdout");
  console.log();
  console.log("OPTIONS (trades):");
  console.log("  --ticker TICKER     Market ticker (required)");
  console.log("  --exchange EXCHANGE Exchange: kalshi (default), kraken, polymarket");
  console.log("  --window WINDOW     Time window (default: 5m). Format: 5m, 10m, 1h");
  console.log("  --detailed          Show individual trade differences");
  console.log("  --env ENV           Override environment");
  console.log();
  console.log("OPTIONS (secmaster):");
  console.log("  --env ENV           Override environment");
  console.log();
  console.log("ENVIRONMENT VARIABLES:");
  console.log("  PROMETHEUS_URL      Prometheus URL (default: http://kube-prometheus-stack-prometheus.observability.svc:9090)");
  console.log("  NATS_URL            NATS server URL (default: nats://nats.nats.svc:4222)");
  console.log("  DATABASE_URL        PostgreSQL connection string (required for daily)");
  console.log("  SSMD_API_URL        API base URL for secmaster (default: http://localhost:3000)");
  console.log("  SSMD_API_KEY        API key for authentication");
  console.log();
  console.log("EXAMPLES:");
  console.log("  ssmd dq daily");
  console.log("  ssmd dq daily --json");
  console.log("  ssmd dq trades --ticker KXBTCD-26FEB0317-T76999.99");
  console.log("  ssmd dq trades --ticker KXBTCD-26FEB0317-T76999.99 --window 10m --detailed");
  console.log("  ssmd dq trades --ticker XBT/USD --exchange kraken --window 5m");
  console.log('  ssmd dq trades --ticker "0x1234..." --exchange polymarket');
  console.log("  ssmd dq secmaster");
  console.log("  ssmd dq secmaster --env dev");
}
