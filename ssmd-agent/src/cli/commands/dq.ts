// dq.ts - Data Quality checks: compare NATS trades with exchange APIs
// Supports Kalshi, Kraken, and Polymarket exchanges

import { createKalshiClient, type KalshiTrade } from "../../lib/api/kalshi.ts";
import { fetchKrakenTrades } from "../../lib/api/kraken-public.ts";
import { getEnvContext } from "../utils/env-context.ts";

// --- Shared types ---

interface DqFlags {
  _: (string | number)[];
  ticker?: string;
  window?: string; // e.g., "5m", "10m", "1h"
  env?: string;
  exchange?: string; // "kalshi" | "kraken" | "polymarket"
  detailed?: boolean;
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
  console.log("  trades          Compare NATS trades with exchange API");
  console.log("  secmaster       Run secmaster data quality checks");
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
  console.log("ENVIRONMENT VARIABLES (secmaster):");
  console.log("  SSMD_API_URL        API base URL (default: http://localhost:3000)");
  console.log("  SSMD_API_KEY        API key for authentication");
  console.log();
  console.log("EXAMPLES:");
  console.log("  ssmd dq trades --ticker KXBTCD-26FEB0317-T76999.99");
  console.log("  ssmd dq trades --ticker KXBTCD-26FEB0317-T76999.99 --window 10m --detailed");
  console.log("  ssmd dq trades --ticker XBT/USD --exchange kraken --window 5m");
  console.log('  ssmd dq trades --ticker "0x1234..." --exchange polymarket');
  console.log("  ssmd dq secmaster");
  console.log("  ssmd dq secmaster --env dev");
}
