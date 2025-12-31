// HTTP server routes
import { listDatasets } from "./handlers/datasets.ts";
import { globalRegistry } from "./metrics.ts";
import { validateApiKey, hasScope } from "./auth.ts";
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
  type Database,
} from "../lib/db/mod.ts";

export const API_VERSION = "1.0.0";

export interface RouteContext {
  dataDir: string;
  db: Database;
}

export interface AuthInfo {
  userId: string;
  userEmail: string;
  scopes: string[];
}

type Handler = (req: Request, ctx: RouteContext) => Promise<Response>;

interface Route {
  method: string;
  pattern: URLPattern;
  handler: Handler;
  requiresAuth: boolean;
  requiredScope?: string;
}

const routes: Route[] = [];

function route(
  method: string,
  path: string,
  handler: Handler,
  requiresAuth = true,
  requiredScope?: string
): void {
  routes.push({
    method,
    pattern: new URLPattern({ pathname: path }),
    handler,
    requiresAuth,
    requiredScope,
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
}, true, "datasets:read");

// Events endpoints
route("GET", "/v1/events", async (req, ctx) => {
  const url = new URL(req.url);
  const events = await listEvents(ctx.db, {
    category: url.searchParams.get("category") ?? undefined,
    status: url.searchParams.get("status") ?? undefined,
    series: url.searchParams.get("series") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ events });
}, true, "secmaster:read");

route("GET", "/v1/events/:ticker", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const event = await getEvent(ctx.db, params.ticker);
  if (!event) {
    return json({ error: "Event not found" }, 404);
  }
  return json(event);
}, true, "secmaster:read");

// Markets endpoints
route("GET", "/v1/markets", async (req, ctx) => {
  const url = new URL(req.url);
  const markets = await listMarkets(ctx.db, {
    category: url.searchParams.get("category") ?? undefined,
    status: url.searchParams.get("status") ?? undefined,
    series: url.searchParams.get("series") ?? undefined,
    eventTicker: url.searchParams.get("event") ?? undefined,
    closingBefore: url.searchParams.get("closing_before") ?? undefined,
    closingAfter: url.searchParams.get("closing_after") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ markets });
}, true, "secmaster:read");

route("GET", "/v1/markets/:ticker", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const market = await getMarket(ctx.db, params.ticker);
  if (!market) {
    return json({ error: "Market not found" }, 404);
  }
  return json(market);
}, true, "secmaster:read");

// Secmaster stats endpoint (combined events + markets)
route("GET", "/v1/secmaster/stats", async (_req, ctx) => {
  const [eventStats, marketStats] = await Promise.all([
    getEventStats(ctx.db),
    getMarketStats(ctx.db),
  ]);
  return json({
    events: eventStats,
    markets: marketStats,
  });
}, true, "secmaster:read");

// Fees endpoints
route("GET", "/v1/fees", async (req, ctx) => {
  const url = new URL(req.url);
  const limit = url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : 100;
  const fees = await listCurrentFees(ctx.db, { limit });
  return json({ fees });
}, true, "secmaster:read");

route("GET", "/v1/fees/stats", async (_req, ctx) => {
  const stats = await getFeeStats(ctx.db);
  return json(stats);
}, true, "secmaster:read");

route("GET", "/v1/fees/:series", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const url = new URL(req.url);
  const asOf = url.searchParams.get("as_of");

  const fee = asOf
    ? await getFeeAsOf(ctx.db, params.series, new Date(asOf))
    : await getCurrentFee(ctx.db, params.series);

  if (!fee) {
    return json({ error: `No fee schedule found for ${params.series}` }, 404);
  }
  return json(fee);
}, true, "secmaster:read");

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

    for (const r of routes) {
      if (req.method !== r.method) continue;

      const match = r.pattern.exec(url);
      if (!match) continue;

      // Check auth if required
      if (r.requiresAuth) {
        const authResult = await validateApiKey(
          req.headers.get("X-API-Key"),
          ctx.db
        );

        if (!authResult.valid) {
          const headers: Record<string, string> = {
            "Content-Type": "application/json",
          };

          if (authResult.rateLimitRemaining !== undefined) {
            headers["X-RateLimit-Remaining"] = authResult.rateLimitRemaining.toString();
            headers["X-RateLimit-Reset"] = authResult.rateLimitResetAt!.toString();
          }

          return new Response(
            JSON.stringify({ error: authResult.error }),
            { status: authResult.status!, headers }
          );
        }

        // Check scope
        if (r.requiredScope && !hasScope(authResult.scopes!, r.requiredScope)) {
          return json({ error: "Insufficient permissions" }, 403);
        }

        // Attach auth info to request for handlers that need it
        Object.defineProperty(req, "auth", {
          value: {
            userId: authResult.userId,
            userEmail: authResult.userEmail,
            scopes: authResult.scopes,
          } as AuthInfo,
        });
      }

      // Add path params to request
      const params = match.pathname.groups;
      Object.defineProperty(req, "params", { value: params });

      return r.handler(req, ctx);
    }

    return json({ error: "Not found" }, 404);
  };
}
