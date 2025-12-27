// ssmd-agent/src/agent/tools.ts
import { tool } from "@langchain/core/tools";
import { z } from "zod";
import { config } from "../config.ts";
import { OrderBookBuilder, type OrderBookState } from "../state/orderbook.ts";
import type { MarketRecord } from "../state/types.ts";
import { runBacktest as executeBacktest } from "../backtest/runner.ts";

const API_TIMEOUT_MS = 10000; // 10 second timeout

async function apiRequest<T>(path: string): Promise<T> {
  const res = await fetch(`${config.dataUrl}${path}`, {
    headers: { "X-API-Key": config.dataApiKey },
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
      feed: z.string().optional().describe("Filter by feed name (e.g., 'kalshi')"),
      from: z.string().optional().describe("Start date YYYY-MM-DD"),
      to: z.string().optional().describe("End date YYYY-MM-DD"),
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
      ticker: z.string().optional().describe("Filter by ticker"),
      type: z.string().optional().describe("Message type: trade, ticker, orderbook"),
      limit: z.number().optional().describe("Max records (default 10)"),
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
  async ({ category, status, series, closing_before, closing_after, limit }) => {
    const params = new URLSearchParams();
    if (category) params.set("category", category);
    if (status) params.set("status", status);
    if (series) params.set("series", series);
    if (closing_before) params.set("closing_before", closing_before);
    if (closing_after) params.set("closing_after", closing_after);
    if (limit) params.set("limit", String(limit));

    const path = `/markets${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "list_markets",
    description: "List markets from secmaster with filters. Returns markets with event metadata.",
    schema: z.object({
      category: z.string().optional().describe("Filter by category (e.g., 'Economics')"),
      status: z.string().optional().describe("Filter by status: open, closed, settled"),
      series: z.string().optional().describe("Filter by series ticker (e.g., 'INXD')"),
      closing_before: z.string().optional().describe("ISO timestamp - markets closing before this time"),
      closing_after: z.string().optional().describe("ISO timestamp - markets closing after this time"),
      limit: z.number().optional().describe("Max results (default 100)"),
    }),
  }
);

export const getMarket = tool(
  async ({ ticker }) => {
    const path = `/markets/${encodeURIComponent(ticker)}`;
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
    const path = `/fees${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "get_fees",
    description: "Get fee schedule (maker/taker fees) for a tier.",
    schema: z.object({
      tier: z.string().optional().describe("Fee tier (default: 'default')"),
    }),
  }
);

export const calendarTools = [getToday];
export const dataTools = [listDatasets, listTickers, sampleData, getSchema, listBuilders, orderbookBuilder];
export const secmasterTools = [listMarkets, getMarket, getFees];
export const allTools = [...calendarTools, ...dataTools, ...secmasterTools, runBacktest, deploySignal];
