// ssmd-agent/src/agent/tools.ts
import { tool } from "@langchain/core/tools";
import { z } from "zod";
import { config } from "../config.ts";

async function apiRequest<T>(path: string): Promise<T> {
  const res = await fetch(`${config.dataUrl}${path}`, {
    headers: { "X-API-Key": config.dataApiKey },
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

export const dataTools = [listDatasets, sampleData, getSchema, listBuilders];
