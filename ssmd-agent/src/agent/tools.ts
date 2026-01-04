// ssmd-agent/src/agent/tools.ts
import { tool } from "@langchain/core/tools";
import { z } from "zod";
import { config } from "../config.ts";
import { OrderBookBuilder, type OrderBookState } from "../state/orderbook.ts";
import { PriceHistoryBuilder, type PriceHistoryState } from "../state/price_history.ts";
import { VolumeProfileBuilder, type VolumeProfileState } from "../state/volume_profile.ts";
import type { MarketRecord } from "../state/types.ts";
import { runBacktest as executeBacktest } from "../backtest/runner.ts";

const API_TIMEOUT_MS = 10000; // 10 second timeout

async function apiRequest<T>(path: string): Promise<T> {
  const res = await fetch(`${config.apiUrl}${path}`, {
    headers: { "X-API-Key": config.apiKey },
    signal: AbortSignal.timeout(API_TIMEOUT_MS),
  }).catch((err) => {
    if (err.name === "TimeoutError") {
      throw new Error(`API timeout after ${API_TIMEOUT_MS / 1000}s - is ssmd-data running?`);
    }
    throw new Error(`API connection failed: ${err.message}`);
  });
  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${await res.text()}`);
  }
  return res.json();
}

export const listDatasets = tool(
  async ({ feed, from, to }) => {
    const params = new URLSearchParams();
    if (feed) params.set("feed", feed);
    if (from) params.set("from", from);
    if (to) params.set("to", to);

    const path = `/datasets${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "list_datasets",
    description: "List available market data datasets. Returns feed, date, record count, ticker count.",
    schema: z.object({
      feed: z.string().optional().nullable().describe("Filter by feed name (e.g., 'kalshi')"),
      from: z.string().optional().nullable().describe("Start date YYYY-MM-DD"),
      to: z.string().optional().nullable().describe("End date YYYY-MM-DD"),
    }),
  }
);

export const sampleData = tool(
  async ({ feed, date, ticker, type, limit }) => {
    const params = new URLSearchParams();
    if (ticker) params.set("ticker", ticker);
    if (type) params.set("type", type);
    if (limit) params.set("limit", String(limit));

    const path = `/datasets/${feed}/${date}/sample${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "sample_data",
    description: "Get sample records from a dataset. Returns raw market data records.",
    schema: z.object({
      feed: z.string().describe("Feed name (e.g., 'kalshi')"),
      date: z.string().describe("Date YYYY-MM-DD"),
      ticker: z.string().optional().nullable().describe("Filter by ticker"),
      type: z.string().optional().nullable().describe("Message type: trade, ticker, orderbook"),
      limit: z.number().optional().nullable().describe("Max records (default 10)"),
    }),
  }
);

export const listTickers = tool(
  async ({ feed, date }) => {
    const path = `/datasets/${feed}/${date}/tickers`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "list_tickers",
    description: "List all tickers available in a dataset for a given feed and date.",
    schema: z.object({
      feed: z.string().describe("Feed name (e.g., 'kalshi')"),
      date: z.string().describe("Date YYYY-MM-DD"),
    }),
  }
);

export const getSchema = tool(
  async ({ feed, type }) => {
    const path = `/schema/${feed}/${type}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "get_schema",
    description: "Get schema for a message type. Shows field names, types, and derived fields.",
    schema: z.object({
      feed: z.string().describe("Feed name"),
      type: z.string().describe("Message type: trade, ticker, orderbook"),
    }),
  }
);

export const listBuilders = tool(
  async () => {
    return JSON.stringify(await apiRequest("/builders"));
  },
  {
    name: "list_builders",
    description: "List available state builders for signal development.",
    schema: z.object({}),
  }
);

export const orderbookBuilder = tool(
  async ({ records }) => {
    const builder = new OrderBookBuilder();
    const snapshots: OrderBookState[] = [];

    for (const record of records as MarketRecord[]) {
      builder.update(record);
      const state = builder.getState();
      // Only add if we have meaningful data
      if (state.ticker) {
        snapshots.push(state);
      }
    }

    return JSON.stringify({
      count: snapshots.length,
      snapshots: snapshots.slice(0, 100), // Limit to prevent huge responses
      summary: snapshots.length > 0 ? {
        ticker: snapshots[0].ticker,
        spreadRange: {
          min: Math.min(...snapshots.map(s => s.spread)),
          max: Math.max(...snapshots.map(s => s.spread)),
        },
      } : null,
    });
  },
  {
    name: "orderbook_builder",
    description: "Process market records through OrderBook state builder. Returns state snapshots with spread calculations.",
    schema: z.object({
      records: z.array(z.any()).describe("Array of market data records from sample_data"),
    }),
  }
);

export const priceHistoryBuilder = tool(
  async ({ records, windowSize }) => {
    const builder = new PriceHistoryBuilder(windowSize ?? 100);
    const snapshots: PriceHistoryState[] = [];

    for (const record of records as MarketRecord[]) {
      builder.update(record);
      const state = builder.getState();
      if (state.ticker && state.tradeCount > 0) {
        snapshots.push(state);
      }
    }

    return JSON.stringify({
      count: snapshots.length,
      snapshots: snapshots.slice(0, 100),
      summary: snapshots.length > 0 ? {
        ticker: snapshots[snapshots.length - 1].ticker,
        priceRange: {
          high: Math.max(...snapshots.map(s => s.high)),
          low: Math.min(...snapshots.filter(s => s.low > 0).map(s => s.low)),
        },
        finalVwap: snapshots[snapshots.length - 1].vwap,
        totalReturns: snapshots[snapshots.length - 1].returns,
      } : null,
    });
  },
  {
    name: "price_history_builder",
    description: "Process trade records through PriceHistory builder. Returns rolling window stats: last, high, low, vwap, returns, volatility.",
    schema: z.object({
      records: z.array(z.any()).describe("Array of trade records from sample_data"),
      windowSize: z.number().optional().nullable().describe("Number of trades in rolling window (default 100)"),
    }),
  }
);

export const volumeProfileBuilder = tool(
  async ({ records, windowMs }) => {
    const builder = new VolumeProfileBuilder(windowMs ?? 300000);
    const snapshots: VolumeProfileState[] = [];

    for (const record of records as MarketRecord[]) {
      builder.update(record);
      const state = builder.getState();
      if (state.ticker && state.tradeCount > 0) {
        snapshots.push(state);
      }
    }

    return JSON.stringify({
      count: snapshots.length,
      snapshots: snapshots.slice(0, 100),
      summary: snapshots.length > 0 ? {
        ticker: snapshots[snapshots.length - 1].ticker,
        finalVolume: {
          contracts: snapshots[snapshots.length - 1].totalVolume,
          dollars: snapshots[snapshots.length - 1].dollarVolume,
        },
        tradeCount: snapshots[snapshots.length - 1].tradeCount,
        windowMs: snapshots[snapshots.length - 1].windowMs,
      } : null,
    });
  },
  {
    name: "volume_profile_builder",
    description: "Process market records through VolumeProfile builder. Tracks contract and USD volume over a sliding time window.",
    schema: z.object({
      records: z.array(z.any()).describe("Array of market records from sample_data"),
      windowMs: z.number().optional().nullable().describe("Time window in milliseconds (default 300000 = 5 min)"),
    }),
  }
);

export const runBacktest = tool(
  async ({ signalCode, states }) => {
    const result = await executeBacktest(signalCode, states);
    return JSON.stringify(result);
  },
  {
    name: "run_backtest",
    description: "Evaluate signal code against state snapshots. Returns fire count, errors, and sample payloads.",
    schema: z.object({
      signalCode: z.string().describe("TypeScript signal code with evaluate() and payload() functions"),
      states: z.array(z.any()).describe("OrderBookState snapshots from orderbook_builder"),
    }),
  }
);

export const deploySignal = tool(
  async ({ code, path }) => {
    // Ensure path is under signals directory
    const fullPath = `${config.signalsPath}/${path}`;

    // Write the file
    await Deno.writeTextFile(fullPath, code);

    // Git add and commit
    const addCmd = new Deno.Command("git", {
      args: ["add", fullPath],
      stdout: "piped",
      stderr: "piped",
    });
    await addCmd.output();

    const commitCmd = new Deno.Command("git", {
      args: ["commit", "-m", `signal: add ${path}`],
      stdout: "piped",
      stderr: "piped",
    });
    const commitResult = await commitCmd.output();

    if (!commitResult.success) {
      const stderr = new TextDecoder().decode(commitResult.stderr);
      return JSON.stringify({ error: `git commit failed: ${stderr}` });
    }

    // Get commit SHA
    const revCmd = new Deno.Command("git", {
      args: ["rev-parse", "HEAD"],
      stdout: "piped",
    });
    const revResult = await revCmd.output();
    const sha = new TextDecoder().decode(revResult.stdout).trim();

    return JSON.stringify({
      path: fullPath,
      sha: sha.slice(0, 7),
      message: `Deployed to ${fullPath}`,
    });
  },
  {
    name: "deploy_signal",
    description: "Write signal file and git commit. Use after successful backtest.",
    schema: z.object({
      code: z.string().describe("Complete TypeScript signal code"),
      path: z.string().describe("Filename within signals/ directory (e.g., 'spread-alert.ts')"),
    }),
  }
);

export const getToday = tool(
  async () => {
    return new Date().toISOString().split("T")[0];
  },
  {
    name: "get_today",
    description: "Get today's date in YYYY-MM-DD format (UTC).",
    schema: z.object({}),
  }
);

export const listMarkets = tool(
  async ({ category, status, series, closing_before, closing_after, as_of, limit }) => {
    const params = new URLSearchParams();
    if (category) params.set("category", category);
    if (status) params.set("status", status);
    if (series) params.set("series", series);
    if (closing_before) params.set("closing_before", closing_before);
    if (closing_after) params.set("closing_after", closing_after);
    if (as_of) params.set("as_of", as_of);
    if (limit) params.set("limit", String(limit));

    const path = `/v1/markets${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "list_markets",
    description: "List markets from secmaster with filters. Supports point-in-time queries to see what markets were tradeable at a specific time.",
    schema: z.object({
      category: z.string().optional().nullable().describe("Filter by category (e.g., 'Economics')"),
      status: z.string().optional().nullable().describe("Filter by status: open, closed, settled"),
      series: z.string().optional().nullable().describe("Filter by series ticker (e.g., 'INXD')"),
      closing_before: z.string().optional().nullable().describe("ISO timestamp - markets closing before this time"),
      closing_after: z.string().optional().nullable().describe("ISO timestamp - markets closing after this time"),
      as_of: z.string().optional().nullable().describe("Point-in-time query: ISO timestamp to see markets that were tradeable at that time (defaults to now)"),
      limit: z.number().optional().nullable().describe("Max results (default 100)"),
    }),
  }
);

export const getMarket = tool(
  async ({ ticker }) => {
    const path = `/v1/markets/${encodeURIComponent(ticker)}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "get_market",
    description: "Get details for a specific market by ticker.",
    schema: z.object({
      ticker: z.string().describe("Market ticker (e.g., 'INXD-25JAN01-B4550')"),
    }),
  }
);

export const getFees = tool(
  async ({ tier }) => {
    const params = new URLSearchParams();
    if (tier) params.set("tier", tier);
    const path = `/v1/fees${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "get_fees",
    description: "Get fee schedule (maker/taker fees) for a tier.",
    schema: z.object({
      tier: z.string().optional().nullable().describe("Fee tier (default: 'default')"),
    }),
  }
);

export const listEvents = tool(
  async ({ category, status, series, as_of, limit }) => {
    const params = new URLSearchParams();
    if (category) params.set("category", category);
    if (status) params.set("status", status);
    if (series) params.set("series", series);
    if (as_of) params.set("as_of", as_of);
    if (limit) params.set("limit", String(limit));

    const path = `/v1/events${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "list_events",
    description: "List events from secmaster with market counts. Supports point-in-time queries to see what events existed at a specific time.",
    schema: z.object({
      category: z.string().optional().nullable().describe("Filter by category (e.g., 'Economics')"),
      status: z.string().optional().nullable().describe("Filter by status: open, closed, settled"),
      series: z.string().optional().nullable().describe("Filter by series ticker (e.g., 'INXD')"),
      as_of: z.string().optional().nullable().describe("Point-in-time query: ISO timestamp to see events that existed at that time (defaults to now)"),
      limit: z.number().optional().nullable().describe("Max results (default 100)"),
    }),
  }
);

export const getEvent = tool(
  async ({ event_ticker }) => {
    const path = `/v1/events/${encodeURIComponent(event_ticker)}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "get_event",
    description: "Get details for a specific event including all its markets.",
    schema: z.object({
      event_ticker: z.string().describe("Event ticker (e.g., 'INXD-25JAN01')"),
    }),
  }
);

export const getFeeSchedule = tool(
  async ({ series_ticker, as_of }) => {
    const params = new URLSearchParams();
    if (as_of) params.set("as_of", as_of);

    const path = `/v1/fees/${encodeURIComponent(series_ticker)}${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "get_fee_schedule",
    description:
      "Get the fee schedule for a series ticker. Returns fee type (quadratic/quadratic_with_maker_fees/flat) and multiplier. Supports point-in-time queries.",
    schema: z.object({
      series_ticker: z
        .string()
        .describe("Series ticker, e.g., 'KXBTC' or 'INXD'"),
      as_of: z
        .string()
        .optional().nullable()
        .describe(
          "Point-in-time query (ISO timestamp), defaults to current schedule"
        ),
    }),
  }
);

export const calendarTools = [getToday];
export const dataTools = [listDatasets, listTickers, sampleData, getSchema, listBuilders, orderbookBuilder, priceHistoryBuilder, volumeProfileBuilder];
export const secmasterTools = [listMarkets, getMarket, getFees, listEvents, getEvent, getFeeSchedule];
export const allTools = [...calendarTools, ...dataTools, ...secmasterTools, runBacktest, deploySignal];
