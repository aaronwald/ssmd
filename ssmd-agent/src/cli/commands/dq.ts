// dq.ts - Data Quality check: compare NATS trades with Kalshi API
// Validates that our pipeline captures all trades correctly

import { createKalshiClient, type KalshiTrade } from "../../lib/api/kalshi.ts";
import { getEnvContext } from "../utils/env-context.ts";

interface DqFlags {
  _: (string | number)[];
  ticker?: string;
  window?: string; // e.g., "5m", "10m", "1h"
  env?: string;
  detailed?: boolean;
}

interface NatsTrade {
  trade_id: string;
  market_ticker: string;
  yes_price: number;
  count: number;
  taker_side: string;
  ts: number; // Unix seconds
}

interface DqResult {
  ticker: string;
  windowStart: Date;
  windowEnd: Date;
  natsCount: number;
  apiCount: number;
  matchedCount: number;
  missingInNats: KalshiTrade[];
  extraInNats: NatsTrade[];
  natsTotalContracts: number;
  apiTotalContracts: number;
}

export async function handleDq(subcommand: string, flags: DqFlags): Promise<void> {
  switch (subcommand) {
    case "trades":
      await runTradesDqCheck(flags);
      break;

    case "help":
    default:
      printDqHelp();
      break;
  }
}

async function runTradesDqCheck(flags: DqFlags): Promise<void> {
  const ticker = flags.ticker;
  if (!ticker) {
    console.error("Error: --ticker is required");
    console.log("Usage: ssmd dq trades --ticker <TICKER> [--window 5m]");
    Deno.exit(1);
  }

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

  console.log(`DQ Check: ${ticker}`);
  console.log(`Window: ${windowStart.toISOString()} to ${windowEnd.toISOString()}`);
  console.log("=".repeat(70));
  console.log();

  // Get context for kubectl commands
  const context = await getEnvContext(flags.env);

  // Fetch trades in parallel
  console.log("Fetching trades...");

  const [natsTrades, apiTrades] = await Promise.all([
    fetchNatsTrades(ticker, windowStart, windowEnd, context.cluster),
    fetchApiTrades(ticker, apiMinTs, apiMaxTs),
  ]);

  // Filter API trades to exact window (without buffer)
  const windowStartSec = Math.floor(windowStart.getTime() / 1000);
  const windowEndSec = Math.floor(windowEnd.getTime() / 1000);
  const apiTradesInWindow = apiTrades.filter((t) => {
    const ts = Math.floor(new Date(t.created_time).getTime() / 1000);
    return ts >= windowStartSec && ts < windowEndSec;
  });

  // Compare by trade_id
  const result = compareTrades(ticker, windowStart, windowEnd, natsTrades, apiTradesInWindow);

  // Print results
  printResults(result, flags.detailed || false);
}

async function fetchNatsTrades(
  ticker: string,
  windowStart: Date,
  windowEnd: Date,
  _cluster: string  // Not used - use current kubectl context
): Promise<NatsTrade[]> {
  // Determine the stream based on ticker category
  const category = inferCategory(ticker);
  const stream = `PROD_KALSHI_${category.toUpperCase()}`;
  const filterSubject = `prod.kalshi.${category.toLowerCase()}.json.trade.${ticker}`;

  // Calculate window duration for consumer delivery policy
  const windowMs = windowEnd.getTime() - windowStart.getTime();
  const windowSec = Math.ceil(windowMs / 1000);
  const sinceDuration = `${windowSec}s`;

  // Generate unique consumer name
  const consumerName = `dq-${Date.now()}`;

  console.log(`  NATS: stream=${stream}, filter=${filterSubject}, since=${sinceDuration}`);

  // Create ephemeral pull consumer and fetch messages in one shell command
  // This ensures we fetch before the consumer is deleted due to inactivity
  // Uses current kubectl context
  const cmd = new Deno.Command("kubectl", {
    args: [
      "exec", "-n", "nats", "deploy/nats-box", "--",
      "sh", "-c",
      `nats consumer add ${stream} ${consumerName} \
        --ephemeral \
        --deliver "${sinceDuration}" \
        --filter "${filterSubject}" \
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

  // Parse output - each trade is on its own line, may have blank lines between
  const lines = output.split("\n");

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || !trimmed.startsWith("{")) continue;

    try {
      const msg = JSON.parse(trimmed);
      if (msg.type === "trade" && msg.msg) {
        const trade: NatsTrade = {
          trade_id: msg.msg.trade_id,
          market_ticker: msg.msg.market_ticker,
          yes_price: msg.msg.yes_price,
          count: msg.msg.count,
          taker_side: msg.msg.taker_side,
          ts: msg.msg.ts,
        };

        // Filter to exact window (consumer --deliver may include slightly more)
        if (trade.ts >= windowStartSec && trade.ts < windowEndSec) {
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

async function fetchApiTrades(
  ticker: string,
  minTs: number,
  maxTs: number
): Promise<KalshiTrade[]> {
  console.log(`  API: fetching trades for ${ticker} [${minTs} - ${maxTs}]`);

  const client = createKalshiClient();
  const trades = await client.fetchAllTrades(ticker, minTs, maxTs);

  console.log(`  API: Found ${trades.length} trades`);
  return trades;
}

function compareTrades(
  ticker: string,
  windowStart: Date,
  windowEnd: Date,
  natsTrades: NatsTrade[],
  apiTrades: KalshiTrade[]
): DqResult {
  // Build sets by trade_id
  const natsById = new Map<string, NatsTrade>();
  for (const t of natsTrades) {
    natsById.set(t.trade_id, t);
  }

  const apiById = new Map<string, KalshiTrade>();
  for (const t of apiTrades) {
    apiById.set(t.trade_id, t);
  }

  // Find matches and differences
  const matched: string[] = [];
  const missingInNats: KalshiTrade[] = [];
  const extraInNats: NatsTrade[] = [];

  // Check API trades against NATS
  for (const [id, trade] of apiById) {
    if (natsById.has(id)) {
      matched.push(id);
    } else {
      missingInNats.push(trade);
    }
  }

  // Check NATS trades against API (find extras)
  for (const [id, trade] of natsById) {
    if (!apiById.has(id)) {
      extraInNats.push(trade);
    }
  }

  // Calculate totals
  const natsTotalContracts = natsTrades.reduce((sum, t) => sum + t.count, 0);
  const apiTotalContracts = apiTrades.reduce((sum, t) => sum + t.count, 0);

  return {
    ticker,
    windowStart,
    windowEnd,
    natsCount: natsTrades.length,
    apiCount: apiTrades.length,
    matchedCount: matched.length,
    missingInNats,
    extraInNats,
    natsTotalContracts,
    apiTotalContracts,
  };
}

function printResults(result: DqResult, detailed: boolean): void {
  console.log();

  const matchRate = result.apiCount > 0
    ? ((result.matchedCount / result.apiCount) * 100).toFixed(1)
    : "N/A";

  const status = result.missingInNats.length === 0 && result.extraInNats.length === 0
    ? "OK"
    : result.missingInNats.length > 0
    ? "WARN (missing)"
    : "WARN (extra)";

  // Summary
  console.log("SUMMARY");
  console.log("-".repeat(40));
  console.log(`  NATS trades:     ${result.natsCount.toString().padStart(6)}    API trades:     ${result.apiCount.toString().padStart(6)}`);
  console.log(`  NATS contracts:  ${result.natsTotalContracts.toString().padStart(6)}    API contracts:  ${result.apiTotalContracts.toString().padStart(6)}`);
  console.log(`  Match rate:      ${matchRate.padStart(5)}%    Status:         ${status}`);
  console.log();

  if (result.missingInNats.length > 0) {
    console.log(`MISSING IN NATS (${result.missingInNats.length} trades)`);
    console.log("-".repeat(40));
    if (detailed) {
      for (const t of result.missingInNats) {
        console.log(`  ${t.trade_id.substring(0, 8)}...  ${t.created_time}  ${t.yes_price}c  ${t.count} contracts  ${t.taker_side}`);
      }
    } else {
      console.log(`  (use --detailed to see individual trades)`);
    }
    console.log();
  }

  if (result.extraInNats.length > 0) {
    console.log(`EXTRA IN NATS (${result.extraInNats.length} trades)`);
    console.log("-".repeat(40));
    if (detailed) {
      for (const t of result.extraInNats) {
        const ts = new Date(t.ts * 1000).toISOString();
        console.log(`  ${t.trade_id.substring(0, 8)}...  ${ts}  ${t.yes_price}c  ${t.count} contracts  ${t.taker_side}`);
      }
    } else {
      console.log(`  (use --detailed to see individual trades)`);
    }
    console.log();
  }
}

function inferCategory(ticker: string): string {
  // Infer category from ticker prefix
  if (ticker.startsWith("KXBTC") || ticker.startsWith("KXETH")) {
    return "crypto";
  } else if (ticker.startsWith("KXNBA") || ticker.startsWith("KXNFL") || ticker.startsWith("KXMLB")) {
    return "sports";
  } else if (ticker.startsWith("INX") || ticker.startsWith("FED") || ticker.startsWith("CPI")) {
    return "economics";
  } else if (ticker.startsWith("PRES") || ticker.startsWith("SEN") || ticker.startsWith("GOV")) {
    return "politics";
  }
  // Default to crypto for now
  return "crypto";
}

function parseWindow(windowStr: string): number | null {
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
  console.log("  trades          Compare NATS trades with Kalshi API");
  console.log();
  console.log("OPTIONS (trades):");
  console.log("  --ticker TICKER   Market ticker (required)");
  console.log("  --window WINDOW   Time window (default: 5m). Format: 5m, 10m, 1h");
  console.log("  --detailed        Show individual trade differences");
  console.log("  --env ENV         Override environment");
  console.log();
  console.log("EXAMPLES:");
  console.log("  ssmd dq trades --ticker KXBTCD-26FEB0317-T76999.99");
  console.log("  ssmd dq trades --ticker KXBTCD-26FEB0317-T76999.99 --window 10m --detailed");
}
