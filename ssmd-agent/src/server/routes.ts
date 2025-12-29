// HTTP server routes
import { listDatasets } from "./handlers/datasets.ts";
import { globalRegistry } from "./metrics.ts";
import type postgres from "postgres";
import {
  listEvents,
  getEvent,
  getEventStats,
  listMarkets,
  getMarket,
  getMarketStats,
  getCurrentFee,
  getFeeAsOf,
  listCurrentFees,
  getFeeStats,
} from "../lib/db/mod.ts";

export const API_VERSION = "1.0.0";

export interface RouteContext {
  apiKey: string;
  dataDir: string;
  sql: postgres.Sql;
}

type Handler = (req: Request, ctx: RouteContext) => Promise<Response>;

interface Route {
  method: string;
  pattern: URLPattern;
  handler: Handler;
  requiresAuth: boolean;
}

const routes: Route[] = [];

function route(
  method: string,
  path: string,
  handler: Handler,
  requiresAuth = true
): void {
  routes.push({
    method,
    pattern: new URLPattern({ pathname: path }),
    handler,
    requiresAuth,
  });
}

// Health endpoint (no auth)
route("GET", "/health", async () => {
  return json({ status: "ok" });
}, false);

// Version endpoint (no auth)
route("GET", "/version", async () => {
  return json({ version: API_VERSION });
}, false);

// Prometheus metrics endpoint (no auth)
route("GET", "/metrics", async () => {
  return new Response(globalRegistry.format(), {
    headers: { "Content-Type": "text/plain; charset=utf-8" },
  });
}, false);

// Datasets endpoint
route("GET", "/datasets", async (req, ctx) => {
  const url = new URL(req.url);
  const feedFilter = url.searchParams.get("feed") ?? undefined;
  const fromDate = url.searchParams.get("from") ?? undefined;
  const toDate = url.searchParams.get("to") ?? undefined;

  const datasets = await listDatasets(ctx.dataDir, feedFilter, fromDate, toDate);
  return json({ datasets });
});

// Events endpoints
route("GET", "/v1/events", async (req, ctx) => {
  const url = new URL(req.url);
  const events = await listEvents(ctx.sql, {
    category: url.searchParams.get("category") ?? undefined,
    status: url.searchParams.get("status") ?? undefined,
    series: url.searchParams.get("series") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ events });
});

route("GET", "/v1/events/:ticker", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const event = await getEvent(ctx.sql, params.ticker);
  if (!event) {
    return json({ error: "Event not found" }, 404);
  }
  return json(event);
});

// Markets endpoints
route("GET", "/v1/markets", async (req, ctx) => {
  const url = new URL(req.url);
  const markets = await listMarkets(ctx.sql, {
    category: url.searchParams.get("category") ?? undefined,
    status: url.searchParams.get("status") ?? undefined,
    series: url.searchParams.get("series") ?? undefined,
    event: url.searchParams.get("event") ?? undefined,
    closing_before: url.searchParams.get("closing_before") ?? undefined,
    closing_after: url.searchParams.get("closing_after") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ markets });
});

route("GET", "/v1/markets/:ticker", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const market = await getMarket(ctx.sql, params.ticker);
  if (!market) {
    return json({ error: "Market not found" }, 404);
  }
  return json(market);
});

// Secmaster stats endpoint (combined events + markets)
route("GET", "/v1/secmaster/stats", async (_req, ctx) => {
  const [eventStats, marketStats] = await Promise.all([
    getEventStats(ctx.sql),
    getMarketStats(ctx.sql),
  ]);
  return json({
    events: eventStats,
    markets: marketStats,
  });
});

// Fees endpoints
route("GET", "/v1/fees", async (req, ctx) => {
  const url = new URL(req.url);
  const limit = url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : 100;
  const fees = await listCurrentFees(ctx.sql, limit);
  return json({ fees });
});

route("GET", "/v1/fees/stats", async (_req, ctx) => {
  const stats = await getFeeStats(ctx.sql);
  return json(stats);
});

route("GET", "/v1/fees/:series", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const url = new URL(req.url);
  const asOf = url.searchParams.get("as_of");

  const fee = asOf
    ? await getFeeAsOf(ctx.sql, params.series, new Date(asOf))
    : await getCurrentFee(ctx.sql, params.series);

  if (!fee) {
    return json({ error: `No fee schedule found for ${params.series}` }, 404);
  }
  return json(fee);
});

// Helper to create JSON response
function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

// Router function
export function createRouter(ctx: RouteContext): (req: Request) => Promise<Response> {
  return async (req: Request) => {
    const url = new URL(req.url);

    for (const route of routes) {
      if (req.method !== route.method) continue;

      const match = route.pattern.exec(url);
      if (!match) continue;

      // Check auth if required
      if (route.requiresAuth) {
        const apiKey = req.headers.get("X-API-Key");
        if (!apiKey || apiKey !== ctx.apiKey) {
          return json({ error: "Unauthorized" }, 401);
        }
      }

      // Add path params to request
      const params = match.pathname.groups;
      Object.defineProperty(req, "params", { value: params });

      return route.handler(req, ctx);
    }

    return json({ error: "Not found" }, 404);
  };
}
