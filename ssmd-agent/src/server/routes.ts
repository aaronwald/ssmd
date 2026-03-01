// HTTP server routes
import { globalRegistry, apiRequestsTotal } from "./metrics.ts";
import { normalizePath } from "./middleware.ts";
import { validateApiKey, hasScope } from "./auth.ts";
import { RequestLogBuffer } from "../lib/db/request-log.ts";
import {
  listEvents,
  getEvent,
  getEventStats,
  listMarkets,
  listMarketsWithSnapshot,
  getMarket,
  getMarketStats,
  getMarketTimeseries,
  getActiveMarketsByCategoryTimeseries,
  getCurrentFee,
  getFeeAsOf,
  listCurrentFees,
  getFeeStats,
  getApiKeyByPrefix,
  getApiKeyByEmail,
  createApiKey,
  listApiKeysByUser,
  listAllApiKeys,
  revokeApiKey,
  disableApiKey,
  enableApiKey,
  updateApiKeyScopes,
  getAllSettings,
  upsertSetting,
  listSeries,
  getSeriesStats,
  listPairs,
  getPair,
  getPairStats,
  getPairSnapshots,
  listConditions,
  listTokensByCategories,
  getCondition,
  getConditionStats,
  listDailyScores,
  getSlaMetrics,
  getGapReports,
  lookupMarketsByIds,
  VALID_FEEDS,
  events,
  markets,
  pairs,
  polymarketConditions,
  billingRates,
  billingLedger,
  apiKeyEvents,
  llmUsageDaily,
  dataAccessLog,
  apiRequestLog,
  type Database,
} from "../lib/db/mod.ts";
import { generateApiKey, invalidateKeyCache } from "../lib/auth/mod.ts";
import { getUsageForPrefix, getTokenUsage, trackTokenUsage } from "../lib/auth/ratelimit.ts";
import { getGuardrailSettings, applyGuardrails, checkModelAllowed } from "../lib/guardrails/mod.ts";
import { getRedis } from "../lib/redis/mod.ts";
import { listParquetFiles, generateSignedUrls, FEED_CONFIG, getCatalog } from "../lib/gcs/mod.ts";
import { logDataAccess } from "../lib/db/mod.ts";
import { query as duckdbQuery } from "../lib/duckdb/mod.ts";
import {
  buildTradeSQL,
  buildPriceSQL,
  buildEventVolumeSQL,
  buildEventMarketsSQL,
  buildTotalVolumeSQL,
  buildTopTickersSQL,
  VOLUME_UNITS,
} from "../lib/duckdb/queries.ts";
import { VALID_DATA_FEEDS, FEED_PATHS } from "../lib/duckdb/feed-config.ts";
import { and, inArray, isNull, eq, gte, lt, lte, desc, sql } from "drizzle-orm";

const USAGE_CACHE_KEY = "cache:keys:usage";
const USAGE_CACHE_TTL = 120; // 2 minutes

export const API_VERSION = "1.0.0";

const OPENROUTER_API_KEY = Deno.env.get("OPENROUTER_API_KEY") ?? "";
const OPENROUTER_BASE_URL = "https://openrouter.ai/api/v1";

export interface RouteContext {
  dataDir: string;
  db: Database;
  authOverride?: (apiKey: string | null, db: Database) => Promise<import("./auth.ts").AuthResult>;
}

export interface AuthInfo {
  userId: string;
  userEmail: string;
  scopes: string[];
  keyPrefix: string;
  allowedFeeds: string[];
  dateRangeStart: string;
  dateRangeEnd: string;
  billable: boolean;
}

type Handler = (req: Request, ctx: RouteContext) => Promise<Response>;
type ApiSurface = "public" | "internal";

interface Route {
  method: string;
  pattern: URLPattern;
  handler: Handler;
  requiresAuth: boolean;
  requiredScope?: string;
  surface: ApiSurface;
}

const routes: Route[] = [];

function route(
  method: string,
  path: string,
  handler: Handler,
  requiresAuth = true,
  requiredScope?: string,
  surface: ApiSurface = "internal"
): void {
  routes.push({
    method,
    pattern: new URLPattern({ pathname: path }),
    handler,
    requiresAuth,
    requiredScope,
    surface,
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

// Legacy /datasets → redirect to /v1/data/catalog
route("GET", "/datasets", async (req) => {
  const url = new URL(req.url);
  const target = new URL("/v1/data/catalog", url.origin);
  // Forward query params
  for (const [key, value] of url.searchParams) {
    target.searchParams.set(key, value);
  }
  return new Response(null, {
    status: 301,
    headers: { "Location": target.toString() },
  });
}, true, "datasets:read", "public");

// Events endpoints
route("GET", "/v1/events", async (req, ctx) => {
  const url = new URL(req.url);
  const events = await listEvents(ctx.db, {
    category: url.searchParams.get("category") ?? undefined,
    status: url.searchParams.get("status") ?? undefined,
    series: url.searchParams.get("series") ?? undefined,
    asOf: url.searchParams.get("as_of") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ events });
}, true, "secmaster:read", "public");

route("GET", "/v1/events/:ticker", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const event = await getEvent(ctx.db, params.ticker);
  if (!event) {
    return json({ error: "Event not found" }, 404);
  }
  return json(event);
}, true, "secmaster:read", "public");

// Markets endpoints
route("GET", "/v1/markets", async (req, ctx) => {
  const url = new URL(req.url);

  // Calculate closingBefore from close_within_hours if provided
  let closingBefore = url.searchParams.get("closing_before") ?? undefined;
  const closeWithinHours = url.searchParams.get("close_within_hours");
  if (closeWithinHours && !closingBefore) {
    const hours = parseInt(closeWithinHours);
    if (!isNaN(hours) && hours > 0) {
      const deadline = new Date(Date.now() + hours * 60 * 60 * 1000);
      closingBefore = deadline.toISOString();
    }
  }

  const options = {
    category: url.searchParams.get("category") ?? undefined,
    status: url.searchParams.get("status") ?? undefined,
    series: url.searchParams.get("series") ?? undefined,
    eventTicker: url.searchParams.get("event") ?? undefined,
    closingBefore,
    closingAfter: url.searchParams.get("closing_after") ?? undefined,
    asOf: url.searchParams.get("as_of") ?? undefined,
    gamesOnly: url.searchParams.get("games_only") === "true",
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  };

  // If include_snapshot=true, return CDC sync metadata (snapshot_time, snapshot_lsn)
  const includeSnapshot = url.searchParams.get("include_snapshot") === "true";
  if (includeSnapshot) {
    const result = await listMarketsWithSnapshot(ctx.db, options);
    return json({
      markets: result.markets,
      snapshot_time: result.snapshotTime,
      snapshot_lsn: result.snapshotLsn,
    });
  }

  const markets = await listMarkets(ctx.db, options);
  return json({ markets });
}, true, "secmaster:read", "public");

// Cross-feed market lookup by IDs (Kalshi tickers, Kraken pair_ids, Polymarket condition/token IDs)
route("GET", "/v1/markets/lookup", async (req, ctx) => {
  const url = new URL(req.url);

  const idsParam = url.searchParams.get("ids");
  if (!idsParam) {
    return json({ error: "ids query parameter is required" }, 400);
  }

  const ids = idsParam.split(",").map((s) => s.trim()).filter(Boolean);
  if (ids.length === 0) {
    return json({ error: "at least one ID is required" }, 400);
  }
  if (ids.length > 100) {
    return json({ error: "maximum 100 IDs per request" }, 400);
  }

  const feed = url.searchParams.get("feed") ?? undefined;
  if (feed && !VALID_FEEDS.includes(feed)) {
    return json({ error: `Invalid feed: ${feed}. Valid feeds: ${VALID_FEEDS.join(", ")}` }, 400);
  }

  const markets = await lookupMarketsByIds(ctx.db, ids, feed);
  return json({ markets });
}, true, "datasets:read", "public");

route("GET", "/v1/markets/:ticker", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const market = await getMarket(ctx.db, params.ticker);
  if (!market) {
    return json({ error: "Market not found" }, 404);
  }
  return json(market);
}, true, "secmaster:read", "public");

// Secmaster stats endpoint (combined events + markets + pairs + conditions)
route("GET", "/v1/secmaster/stats", async (_req, ctx) => {
  const [eventStats, marketStats, pairStats, conditionStats] = await Promise.all([
    getEventStats(ctx.db),
    getMarketStats(ctx.db),
    getPairStats(ctx.db),
    getConditionStats(ctx.db),
  ]);
  return json({
    events: eventStats,
    markets: marketStats,
    pairs: pairStats,
    conditions: conditionStats,
  });
}, true, "secmaster:read", "public");

// Market activity timeseries (added/closed per day)
route("GET", "/v1/secmaster/markets/timeseries", async (req, ctx) => {
  const url = new URL(req.url);
  const days = url.searchParams.get("days")
    ? parseInt(url.searchParams.get("days")!)
    : 30;
  const timeseries = await getMarketTimeseries(ctx.db, days);
  return json({ timeseries });
}, true, "secmaster:read", "public");

// Active markets by category over time
route("GET", "/v1/secmaster/markets/active-by-category", async (req, ctx) => {
  const url = new URL(req.url);
  const days = url.searchParams.get("days")
    ? parseInt(url.searchParams.get("days")!)
    : 7;
  const timeseries = await getActiveMarketsByCategoryTimeseries(ctx.db, days);
  return json({ timeseries });
}, true, "secmaster:read", "public");

// Lifecycle search — query markets by status across all exchanges
route("GET", "/v1/secmaster/lifecycle", async (req, ctx) => {
  const url = new URL(req.url);
  const status = url.searchParams.get("status") ?? undefined;
  const since = url.searchParams.get("since") ?? undefined;
  const feed = url.searchParams.get("feed") ?? undefined;
  const limit = url.searchParams.get("limit")
    ? Math.min(Math.max(parseInt(url.searchParams.get("limit")!), 1), 500)
    : 100;

  if (feed && !VALID_FEEDS.includes(feed)) {
    return json({ error: `Invalid feed: ${feed}. Valid feeds: ${VALID_FEEDS.join(", ")}` }, 400);
  }

  // deno-lint-ignore no-explicit-any
  const results: any[] = [];

  // Query Kalshi markets
  if (!feed || feed === "kalshi") {
    const conds = [isNull(markets.deletedAt)];
    if (status) conds.push(eq(markets.status, status));
    if (since) conds.push(gte(markets.updatedAt, new Date(since)));

    const kalshiRows = await ctx.db
      .select({
        id: markets.ticker,
        title: markets.title,
        status: markets.status,
        updatedAt: markets.updatedAt,
      })
      .from(markets)
      .where(and(...conds))
      .orderBy(desc(markets.updatedAt))
      .limit(limit);

    for (const row of kalshiRows) {
      results.push({ exchange: "kalshi", ...row });
    }
  }

  // Query Kraken pairs
  if (!feed || feed === "kraken-futures") {
    const conds = [isNull(pairs.deletedAt)];
    if (status) conds.push(eq(pairs.status, status));
    if (since) conds.push(gte(pairs.updatedAt, new Date(since)));

    const krakenRows = await ctx.db
      .select({
        id: pairs.pairId,
        title: pairs.wsName,
        status: pairs.status,
        updatedAt: pairs.updatedAt,
      })
      .from(pairs)
      .where(and(...conds))
      .orderBy(desc(pairs.updatedAt))
      .limit(limit);

    for (const row of krakenRows) {
      results.push({ exchange: "kraken-futures", ...row });
    }
  }

  // Query Polymarket conditions
  if (!feed || feed === "polymarket") {
    const conds = [isNull(polymarketConditions.deletedAt)];
    if (status) conds.push(eq(polymarketConditions.status, status));
    if (since) conds.push(gte(polymarketConditions.updatedAt, new Date(since)));

    const polyRows = await ctx.db
      .select({
        id: polymarketConditions.conditionId,
        title: polymarketConditions.question,
        status: polymarketConditions.status,
        updatedAt: polymarketConditions.updatedAt,
      })
      .from(polymarketConditions)
      .where(and(...conds))
      .orderBy(desc(polymarketConditions.updatedAt))
      .limit(limit);

    for (const row of polyRows) {
      results.push({ exchange: "polymarket", ...row });
    }
  }

  // Sort combined results by updatedAt descending, then trim to limit
  results.sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime());

  return json({ results: results.slice(0, limit) });
}, true, "secmaster:read", "public");

// Series endpoints
route("GET", "/v1/series", async (req, ctx) => {
  const url = new URL(req.url);
  const series = await listSeries({
    category: url.searchParams.get("category") ?? undefined,
    tag: url.searchParams.get("tag") ?? undefined,
    gamesOnly: url.searchParams.get("games_only") === "true",
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ series });
}, true, "secmaster:read", "public");

route("GET", "/v1/series/stats", async (_req, _ctx) => {
  const stats = await getSeriesStats();
  return json({ stats });
}, true, "secmaster:read", "public");

// Pairs endpoints
route("GET", "/v1/pairs", async (req, ctx) => {
  const url = new URL(req.url);
  const pairs = await listPairs(ctx.db, {
    exchange: url.searchParams.get("exchange") ?? undefined,
    marketType: url.searchParams.get("market_type") ?? undefined,
    base: url.searchParams.get("base") ?? undefined,
    quote: url.searchParams.get("quote") ?? undefined,
    status: url.searchParams.get("status") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ pairs });
}, true, "secmaster:read", "public");

route("GET", "/v1/pairs/stats", async (_req, ctx) => {
  const stats = await getPairStats(ctx.db);
  return json({ stats });
}, true, "secmaster:read", "public");

route("GET", "/v1/pairs/:pairId/snapshots", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const url = new URL(req.url);
  const snapshots = await getPairSnapshots(ctx.db, params.pairId, {
    from: url.searchParams.get("from") ?? undefined,
    to: url.searchParams.get("to") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ snapshots });
}, true, "secmaster:read", "public");

route("GET", "/v1/pairs/:pairId", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const pair = await getPair(ctx.db, params.pairId);
  if (!pair) {
    return json({ error: "Pair not found" }, 404);
  }
  return json(pair);
}, true, "secmaster:read", "public");

// Conditions endpoints (Polymarket)
route("GET", "/v1/conditions", async (req, ctx) => {
  const url = new URL(req.url);
  const conditions = await listConditions(ctx.db, {
    category: url.searchParams.get("category") ?? undefined,
    status: url.searchParams.get("status") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ conditions });
}, true, "secmaster:read", "public");

route("GET", "/v1/conditions/:conditionId", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const result = await getCondition(ctx.db, params.conditionId);
  if (!result) {
    return json({ error: "Condition not found" }, 404);
  }
  return json(result);
}, true, "secmaster:read", "public");

// Polymarket token listing (for connector subscription filtering)
route("GET", "/v1/polymarket/tokens", async (req, ctx) => {
  const url = new URL(req.url);
  const categoryParam = url.searchParams.get("category");
  if (!categoryParam) {
    return json({ error: "category query parameter is required" }, 400);
  }
  const categories = categoryParam.split(",").map((c) => c.trim()).filter(Boolean);
  if (categories.length === 0) {
    return json({ error: "at least one category is required" }, 400);
  }
  const status = url.searchParams.get("status") ?? "active";
  const minVolumeParam = url.searchParams.get("minVolume");
  const minVolume = minVolumeParam ? Number(minVolumeParam) : undefined;
  const qParam = url.searchParams.get("q");
  const questionKeywords = qParam
    ? qParam.split(",").map((s) => s.trim()).filter(Boolean)
    : undefined;
  const tokens = await listTokensByCategories(ctx.db, {
    categories,
    status,
    minVolume,
    questionKeywords,
  });
  return json({ tokens });
}, true, "secmaster:read", "public");

// Fees endpoints
route("GET", "/v1/fees", async (req, ctx) => {
  const url = new URL(req.url);
  const limit = url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : 100;
  const fees = await listCurrentFees(ctx.db, { limit });
  return json({ fees });
}, true, "secmaster:read", "public");

route("GET", "/v1/fees/stats", async (_req, ctx) => {
  const stats = await getFeeStats(ctx.db);
  return json(stats);
}, true, "secmaster:read", "public");

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
}, true, "secmaster:read", "public");

// Health check endpoints
route("GET", "/v1/health/daily", async (req, ctx) => {
  const url = new URL(req.url);
  const scores = await listDailyScores(ctx.db, {
    feed: url.searchParams.get("feed") ?? undefined,
    from: url.searchParams.get("from") ?? undefined,
    to: url.searchParams.get("to") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ scores });
}, true, "secmaster:read", "public");

route("GET", "/v1/health/sla", async (req, ctx) => {
  const url = new URL(req.url);
  const windowDays = url.searchParams.get("window_days")
    ? parseInt(url.searchParams.get("window_days")!)
    : undefined;
  const metrics = await getSlaMetrics(ctx.db, { windowDays });
  return json({ metrics });
}, true, "secmaster:read", "public");

route("GET", "/v1/health/gaps", async (req, ctx) => {
  const url = new URL(req.url);
  const feed = url.searchParams.get("feed");
  if (!feed) {
    return json({ error: "feed query parameter is required" }, 400);
  }
  const gaps = await getGapReports(ctx.db, {
    feed,
    from: url.searchParams.get("from") ?? undefined,
    to: url.searchParams.get("to") ?? undefined,
  });
  return json({ gaps });
}, true, "secmaster:read", "public");

// Key management endpoints
const VALID_SCOPES = [
  "secmaster:read", "datasets:read", "signals:read", "signals:write",
  "admin:read", "admin:write", "llm:chat", "billing:read", "billing:write",
  "harman:read", "harman:write", "harman:admin",
];

route("POST", "/v1/keys", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const body = await req.json() as {
    name: string;
    scopes: string[];
    rateLimitTier?: string;
    environment?: "live" | "test";
    userEmail?: string;
    expiresInHours?: number;
    allowedFeeds?: string[];
    dateRangeStart?: string;
    dateRangeEnd?: string;
    billable?: boolean;
  };

  // Validate required fields
  if (!body.name || !body.scopes || body.scopes.length === 0) {
    return json({ error: "name and scopes are required" }, 400);
  }

  // Validate scopes
  for (const scope of body.scopes) {
    if (!VALID_SCOPES.includes(scope)) {
      return json({ error: `Invalid scope: ${scope}` }, 400);
    }
  }

  // Validate allowed feeds (required)
  if (!body.allowedFeeds || body.allowedFeeds.length === 0) {
    return json({ error: "allowedFeeds is required and must not be empty" }, 400);
  }
  for (const feed of body.allowedFeeds) {
    if (!FEED_CONFIG[feed]) {
      return json({ error: `Invalid feed: ${feed}. Valid feeds: ${Object.keys(FEED_CONFIG).join(", ")}` }, 400);
    }
  }

  // Validate date range (required)
  const dateRegex = /^\d{4}-\d{2}-\d{2}$/;
  if (!body.dateRangeStart || !body.dateRangeEnd) {
    return json({ error: "dateRangeStart and dateRangeEnd are required (YYYY-MM-DD)" }, 400);
  }
  if (!dateRegex.test(body.dateRangeStart) || !dateRegex.test(body.dateRangeEnd)) {
    return json({ error: "dateRangeStart and dateRangeEnd must be YYYY-MM-DD format" }, 400);
  }
  if (new Date(body.dateRangeStart) > new Date(body.dateRangeEnd)) {
    return json({ error: "dateRangeStart must be before or equal to dateRangeEnd" }, 400);
  }

  // Validate expiration
  let expiresAt: Date | undefined;
  if (body.expiresInHours !== undefined) {
    if (body.expiresInHours < 1 || body.expiresInHours > 720) {
      return json({ error: "expiresInHours must be between 1 and 720 (30 days)" }, 400);
    }
    expiresAt = new Date(Date.now() + body.expiresInHours * 3600_000);
  }

  const { fullKey, prefix, hash } = await generateApiKey(body.environment ?? "live");

  const apiKey = await createApiKey(ctx.db, {
    id: crypto.randomUUID(),
    userId: auth.userId,
    userEmail: body.userEmail ?? auth.userEmail,
    keyPrefix: prefix,
    keyHash: hash,
    name: body.name,
    scopes: body.scopes,
    rateLimitTier: body.rateLimitTier ?? "standard",
    expiresAt: expiresAt ?? null,
    allowedFeeds: body.allowedFeeds,
    dateRangeStart: body.dateRangeStart,
    dateRangeEnd: body.dateRangeEnd,
    billable: body.billable ?? true,
  });

  // Return full key ONCE
  return json({
    key: fullKey,
    prefix: apiKey.keyPrefix,
    name: apiKey.name,
    scopes: apiKey.scopes,
    rateLimitTier: apiKey.rateLimitTier,
    createdAt: apiKey.createdAt,
    expiresAt: apiKey.expiresAt,
    allowedFeeds: apiKey.allowedFeeds,
    dateRangeStart: apiKey.dateRangeStart,
    dateRangeEnd: apiKey.dateRangeEnd,
  }, 201);
}, true, "admin:write");

route("GET", "/v1/keys", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const url = new URL(req.url);
  const includeRevoked = url.searchParams.get("include_revoked") === "true";

  let keys;
  if (hasScope(auth.scopes, "admin:read")) {
    keys = await listAllApiKeys(ctx.db, includeRevoked);
  } else {
    keys = await listApiKeysByUser(ctx.db, auth.userId);
  }

  // Never return the hash
  return json({
    keys: keys.map((k) => ({
      prefix: k.keyPrefix,
      name: k.name,
      userId: k.userId,
      userEmail: k.userEmail,
      scopes: k.scopes,
      rateLimitTier: k.rateLimitTier,
      lastUsedAt: k.lastUsedAt,
      createdAt: k.createdAt,
      expiresAt: k.expiresAt,
      revokedAt: k.revokedAt,
      disabledAt: k.disabledAt,
      billable: k.billable,
      allowedFeeds: k.allowedFeeds,
      dateRangeStart: k.dateRangeStart,
      dateRangeEnd: k.dateRangeEnd,
    })),
  });
}, true, "admin:read");

route("DELETE", "/v1/keys/:prefix", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const params = (req as Request & { params: Record<string, string> }).params;

  // Check if key exists
  const key = await getApiKeyByPrefix(ctx.db, params.prefix);
  if (!key) {
    return json({ error: "Key not found" }, 404);
  }

  // Check ownership or admin
  const isOwner = key.userId === auth.userId;
  const isAdmin = hasScope(auth.scopes, "admin:write");

  if (!isOwner && !isAdmin) {
    return json({ error: "Forbidden" }, 403);
  }

  const revoked = await revokeApiKey(ctx.db, params.prefix, auth.userEmail);
  if (revoked) {
    await invalidateKeyCache(params.prefix);
  }

  return json({ revoked });
}, true, "admin:read");

route("PATCH", "/v1/keys/:prefix", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const params = (req as Request & { params: Record<string, string> }).params;

  // Admin only
  if (!hasScope(auth.scopes, "admin:write")) {
    return json({ error: "Forbidden" }, 403);
  }

  const key = await getApiKeyByPrefix(ctx.db, params.prefix);
  if (!key) {
    return json({ error: "Key not found" }, 404);
  }

  const body = await req.json() as { scopes?: string[] };

  if (!body.scopes || !Array.isArray(body.scopes) || body.scopes.length === 0) {
    return json({ error: "scopes array is required" }, 400);
  }

  // Validate scopes
  for (const s of body.scopes) {
    if (!VALID_SCOPES.includes(s) && s !== "*") {
      return json({ error: `Invalid scope: ${s}. Valid: ${VALID_SCOPES.join(", ")}, *` }, 400);
    }
  }

  const updated = await updateApiKeyScopes(ctx.db, params.prefix, body.scopes, auth.userEmail);
  if (updated) {
    await invalidateKeyCache(params.prefix);
  }

  return json({
    prefix: params.prefix,
    scopes: body.scopes,
    updated: !!updated,
  });
}, true, "admin:write");

// Disable a key (temporary suspension)
route("POST", "/v1/keys/:prefix/disable", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const params = (req as Request & { params: Record<string, string> }).params;

  const key = await getApiKeyByPrefix(ctx.db, params.prefix);
  if (!key) {
    return json({ error: "Key not found" }, 404);
  }

  if (key.disabledAt) {
    return json({ error: "Key is already disabled" }, 409);
  }

  const disabled = await disableApiKey(ctx.db, params.prefix, auth.userEmail);
  if (disabled) {
    await invalidateKeyCache(params.prefix);
  }

  return json({ disabled });
}, true, "admin:write");

// Enable a previously disabled key
route("POST", "/v1/keys/:prefix/enable", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const params = (req as Request & { params: Record<string, string> }).params;

  const key = await getApiKeyByPrefix(ctx.db, params.prefix);
  if (!key) {
    return json({ error: "Key not found" }, 404);
  }

  if (!key.disabledAt) {
    return json({ error: "Key is not disabled" }, 409);
  }

  const enabled = await enableApiKey(ctx.db, params.prefix, auth.userEmail);
  if (enabled) {
    await invalidateKeyCache(params.prefix);
  }

  return json({ enabled });
}, true, "admin:write");

// Usage stats endpoint - get rate limit and token usage for all keys
// Cached for 2 minutes because SCAN on large key sets is slow
route("GET", "/v1/keys/usage", async (_req, ctx) => {
  const redis = await getRedis();

  // Check cache first
  const cached = await redis.get(USAGE_CACHE_KEY);
  if (cached) {
    return new Response(cached, {
      headers: { "Content-Type": "application/json", "X-Cache": "HIT" },
    });
  }

  const keys = await listAllApiKeys(ctx.db);

  const usage = await Promise.all(
    keys.map(async (k) => {
      const [rateLimit, tokens] = await Promise.all([
        getUsageForPrefix(k.keyPrefix, k.rateLimitTier),
        getTokenUsage(k.keyPrefix),
      ]);
      return { ...rateLimit, ...tokens };
    })
  );

  const responseBody = JSON.stringify({ usage });

  // Cache the result
  await redis.set(USAGE_CACHE_KEY, responseBody, { ex: USAGE_CACHE_TTL });

  return new Response(responseBody, {
    headers: { "Content-Type": "application/json", "X-Cache": "MISS" },
  });
}, true, "admin:read");

// Per-key API request counts from in-memory Prometheus counter
route("GET", "/v1/keys/requests", async () => {
  const entries = apiRequestsTotal.entries();

  // Group by key_prefix
  const byKey: Record<string, { total: number; endpoints: Record<string, number> }> = {};
  for (const { labels, value } of entries) {
    const prefix = labels.key_prefix;
    if (!byKey[prefix]) {
      byKey[prefix] = { total: 0, endpoints: {} };
    }
    byKey[prefix].total += value;
    const endpoint = `${labels.method}:${labels.path}`;
    byKey[prefix].endpoints[endpoint] = (byKey[prefix].endpoints[endpoint] ?? 0) + value;
  }

  const keys = Object.entries(byKey).map(([keyPrefix, data]) => ({
    keyPrefix,
    totalRequests: data.total,
    endpoints: Object.entries(data.endpoints)
      .map(([endpoint, count]) => ({ endpoint, count }))
      .sort((a, b) => b.count - a.count),
  })).sort((a, b) => b.totalRequests - a.totalRequests);

  return json({ sincePodStart: true, keys });
}, true, "admin:read");

// Auth validation endpoint
route("GET", "/v1/auth/validate", (req, _ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  return Promise.resolve(json({ valid: true, scopes: auth.scopes, key_prefix: auth.keyPrefix }));
}, true, undefined, "internal");

// Auth email lookup endpoint (for CF JWT → API key resolution)
route("GET", "/v1/auth/lookup", async (req, ctx) => {
  const url = new URL(req.url);
  const email = url.searchParams.get("email");
  if (!email) {
    return json({ error: "email query parameter is required" }, 400);
  }

  const key = await getApiKeyByEmail(ctx.db, email);
  if (!key) {
    return json({ found: false });
  }

  return json({ found: true, key_prefix: key.keyPrefix, scopes: key.scopes });
}, true, "admin:read", "internal");

// Settings endpoints
route("GET", "/v1/settings", async (_req, ctx) => {
  const allSettings = await getAllSettings(ctx.db);
  return json({ settings: allSettings });
}, true, "admin:read");

route("PUT", "/v1/settings/:key", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const body = await req.json() as { value: unknown };

  if (body.value === undefined) {
    return json({ error: "value is required" }, 400);
  }

  const setting = await upsertSetting(ctx.db, params.key, body.value);
  return json(setting);
}, true, "admin:write");

// --- Billing helpers ---

// Map path to endpoint tier for billing rate lookup
function endpointTier(_method: string, path: string): string {
  if (path.startsWith("/v1/data/download")) return "data_download";
  if (path.startsWith("/v1/data/")) return "data_query";
  if (path.startsWith("/v1/markets/lookup")) return "market_lookup";
  if (path.startsWith("/v1/secmaster/")) return "secmaster";
  if (path.startsWith("/v1/events") || path.startsWith("/v1/markets") || path.startsWith("/v1/series") ||
      path.startsWith("/v1/pairs") || path.startsWith("/v1/conditions") || path.startsWith("/v1/fees")) return "secmaster";
  if (path.startsWith("/v1/chat/completions")) return "llm_chat";
  return "data_query";
}

// Compute billing from api_request_log + billing_rates for a date range
async function computeBillingForPeriod(
  db: Database,
  keyPrefix: string | null,
  from: string,  // YYYY-MM-DD
  to: string,    // YYYY-MM-DD (exclusive)
): Promise<Array<{ keyPrefix: string; endpoint: string; requests: number; bytes: number; errors: number; costUsd: number }>> {
  // Query api_request_log for the period
  const conditions = [
    gte(apiRequestLog.createdAt, new Date(from + "T00:00:00Z")),
    lt(apiRequestLog.createdAt, new Date(to + "T00:00:00Z")),
  ];
  if (keyPrefix) {
    conditions.push(eq(apiRequestLog.keyPrefix, keyPrefix));
  }

  const rows = await db.select().from(apiRequestLog).where(and(...conditions));
  if (rows.length === 0) return [];

  // Load current billing rates
  const rates = await db
    .select()
    .from(billingRates)
    .where(and(
      lte(billingRates.effectiveFrom, new Date(to + "T00:00:00Z")),
      isNull(billingRates.effectiveTo),
    ));

  // Build rate lookup: key-specific first, then global fallback
  const rateLookup = new Map<string, { perReq: number; perMb: number }>();
  for (const r of rates) {
    const key = r.keyPrefix ? `${r.keyPrefix}:${r.endpointTier}` : `_global:${r.endpointTier}`;
    rateLookup.set(key, { perReq: parseFloat(r.ratePerRequest), perMb: parseFloat(r.ratePerMb) });
  }
  function getRate(kp: string, tier: string): { perReq: number; perMb: number } {
    return rateLookup.get(`${kp}:${tier}`) ?? rateLookup.get(`_global:${tier}`) ?? { perReq: 0, perMb: 0 };
  }

  // Group by (key_prefix, endpoint_tier)
  const groups = new Map<string, { keyPrefix: string; endpoint: string; count: number; bytes: number; errors: number }>();
  for (const row of rows) {
    const tier = endpointTier(row.method, row.path);
    const groupKey = `${row.keyPrefix}:${tier}`;
    const existing = groups.get(groupKey);
    if (existing) {
      existing.count++;
      existing.bytes += row.responseBytes ?? 0;
      if (row.statusCode >= 400) existing.errors++;
    } else {
      groups.set(groupKey, {
        keyPrefix: row.keyPrefix, endpoint: tier, count: 1,
        bytes: row.responseBytes ?? 0, errors: row.statusCode >= 400 ? 1 : 0,
      });
    }
  }

  // Compute costs
  const details: Array<{ keyPrefix: string; endpoint: string; requests: number; bytes: number; errors: number; costUsd: number }> = [];
  for (const [, group] of groups) {
    const rate = getRate(group.keyPrefix, group.endpoint);
    const costUsd = group.count * rate.perReq + (group.bytes / (1024 * 1024)) * rate.perMb;
    details.push({
      keyPrefix: group.keyPrefix, endpoint: group.endpoint,
      requests: group.count, bytes: group.bytes, errors: group.errors,
      costUsd: Math.round(costUsd * 1_000_000) / 1_000_000,
    });
  }

  return details;
}

// Billing summary for a single key
route("GET", "/v1/billing/summary", async (req, ctx) => {
  const url = new URL(req.url);
  const keyPrefix = url.searchParams.get("key_prefix");
  if (!keyPrefix) {
    return json({ error: "key_prefix query parameter is required" }, 400);
  }

  const monthParam = url.searchParams.get("month");
  const now = new Date();
  const month = monthParam ?? `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}`;

  // Parse month into date range
  const monthMatch = month.match(/^(\d{4})-(\d{2})$/);
  if (!monthMatch) {
    return json({ error: "month must be YYYY-MM format" }, 400);
  }
  const [, year, mon] = monthMatch;
  const from = `${year}-${mon}-01`;
  const nextMonth = parseInt(mon) === 12
    ? `${parseInt(year) + 1}-01-01`
    : `${year}-${String(parseInt(mon) + 1).padStart(2, "0")}-01`;

  // Compute from api_request_log + billing_rates
  const computed = await computeBillingForPeriod(ctx.db, keyPrefix, from, nextMonth);

  // Aggregate by endpoint
  const byEndpoint: Record<string, { count: number; bytes: number; errors: number }> = {};
  let totalRequests = 0;
  let totalBytes = 0;
  let totalErrors = 0;
  for (const row of computed) {
    const ep = row.endpoint;
    if (!byEndpoint[ep]) byEndpoint[ep] = { count: 0, bytes: 0, errors: 0 };
    byEndpoint[ep].count += row.requests;
    byEndpoint[ep].bytes += row.bytes;
    byEndpoint[ep].errors += row.errors;
    totalRequests += row.requests;
    totalBytes += row.bytes;
    totalErrors += row.errors;
  }

  // Query llm_usage_daily
  const llmRows = await ctx.db
    .select()
    .from(llmUsageDaily)
    .where(and(
      eq(llmUsageDaily.keyPrefix, keyPrefix),
      gte(llmUsageDaily.date, from),
      lt(llmUsageDaily.date, nextMonth)
    ));

  const llmByModel: Record<string, { prompt: number; completion: number; requests: number; cost: string }> = {};
  let llmTotalRequests = 0;
  let llmTotalTokens = 0;
  let llmTotalCost = 0;
  for (const row of llmRows) {
    if (!llmByModel[row.model]) {
      llmByModel[row.model] = { prompt: 0, completion: 0, requests: 0, cost: "0" };
    }
    llmByModel[row.model].prompt += row.promptTokens;
    llmByModel[row.model].completion += row.completionTokens;
    llmByModel[row.model].requests += row.requests;
    llmByModel[row.model].cost = String(parseFloat(llmByModel[row.model].cost) + parseFloat(row.costUsd));
    llmTotalRequests += row.requests;
    llmTotalTokens += row.promptTokens + row.completionTokens;
    llmTotalCost += parseFloat(row.costUsd);
  }

  // Query key events
  const keyEvents = await ctx.db
    .select()
    .from(apiKeyEvents)
    .where(and(
      eq(apiKeyEvents.keyPrefix, keyPrefix),
      gte(apiKeyEvents.createdAt, new Date(from)),
      lt(apiKeyEvents.createdAt, new Date(nextMonth))
    ));

  return json({
    keyPrefix,
    period: { month, from, to: nextMonth },
    apiRequests: {
      total: totalRequests,
      totalBytes,
      totalErrors,
      byEndpoint,
    },
    llmUsage: {
      totalRequests: llmTotalRequests,
      totalTokens: llmTotalTokens,
      costUsd: Math.round(llmTotalCost * 1_000_000) / 1_000_000,
      byModel: Object.entries(llmByModel).map(([model, data]) => ({
        model,
        ...data,
      })),
    },
    keyEvents: keyEvents.map((e) => ({
      type: e.eventType,
      actor: e.actor,
      at: e.createdAt,
    })),
  });
}, true, "billing:read");

// Billing report for all billable keys
route("GET", "/v1/billing/report", async (req, ctx) => {
  const url = new URL(req.url);
  const monthParam = url.searchParams.get("month");
  const now = new Date();
  const month = monthParam ?? `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}`;

  const monthMatch = month.match(/^(\d{4})-(\d{2})$/);
  if (!monthMatch) {
    return json({ error: "month must be YYYY-MM format" }, 400);
  }
  const [, year, mon] = monthMatch;
  const from = `${year}-${mon}-01`;
  const nextMonth = parseInt(mon) === 12
    ? `${parseInt(year) + 1}-01-01`
    : `${year}-${String(parseInt(mon) + 1).padStart(2, "0")}-01`;

  // Get all billable keys
  const allKeys = await listAllApiKeys(ctx.db, true);
  const billableKeys = allKeys.filter((k) => k.billable);

  // Compute from api_request_log + billing_rates (all keys)
  const computed = await computeBillingForPeriod(ctx.db, null, from, nextMonth);

  // Group by key_prefix
  const byKey: Record<string, { requests: number; bytes: number; errors: number }> = {};
  for (const row of computed) {
    if (!byKey[row.keyPrefix]) byKey[row.keyPrefix] = { requests: 0, bytes: 0, errors: 0 };
    byKey[row.keyPrefix].requests += row.requests;
    byKey[row.keyPrefix].bytes += row.bytes;
    byKey[row.keyPrefix].errors += row.errors;
  }

  const report = billableKeys.map((k) => ({
    keyPrefix: k.keyPrefix,
    keyName: k.name,
    userEmail: k.userEmail,
    totalRequests: byKey[k.keyPrefix]?.requests ?? 0,
    totalBytes: byKey[k.keyPrefix]?.bytes ?? 0,
    totalErrors: byKey[k.keyPrefix]?.errors ?? 0,
  }));

  return json({ month, from, to: nextMonth, keys: report });
}, true, "billing:read");

// Billing export as CSV
route("GET", "/v1/billing/export", async (req, ctx) => {
  const url = new URL(req.url);
  const monthParam = url.searchParams.get("month");
  const now = new Date();
  const month = monthParam ?? `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}`;

  const monthMatch = month.match(/^(\d{4})-(\d{2})$/);
  if (!monthMatch) {
    return json({ error: "month must be YYYY-MM format" }, 400);
  }
  const [, year, mon] = monthMatch;
  const from = `${year}-${mon}-01`;
  const nextMonth = parseInt(mon) === 12
    ? `${parseInt(year) + 1}-01-01`
    : `${year}-${String(parseInt(mon) + 1).padStart(2, "0")}-01`;

  // Compute from api_request_log + billing_rates (all keys)
  const computed = await computeBillingForPeriod(ctx.db, null, from, nextMonth);

  // Build CSV
  const header = "key_prefix,endpoint,request_count,response_bytes,error_count,cost_usd";
  const lines = computed.map((r) =>
    `${r.keyPrefix},${r.endpoint},${r.requests},${r.bytes},${r.errors},${r.costUsd}`
  );
  const csv = [header, ...lines].join("\n");

  return new Response(csv, {
    headers: {
      "Content-Type": "text/csv",
      "Content-Disposition": `attachment; filename="billing-${month}.csv"`,
    },
  });
}, true, "billing:read");

// Run billing aggregation for a date (writes debit entries to billing_ledger)
route("POST", "/v1/billing/aggregate", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const body = await req.json().catch(() => ({})) as {
    date?: string;
    dry_run?: boolean;
  };

  // Default: yesterday
  let targetDate: string;
  if (body.date) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(body.date)) {
      return json({ error: "date must be YYYY-MM-DD format" }, 400);
    }
    targetDate = body.date;
  } else {
    const d = new Date();
    d.setUTCDate(d.getUTCDate() - 1);
    targetDate = d.toISOString().slice(0, 10);
  }
  const dryRun = Boolean(body.dry_run);

  const nextDate = new Date(targetDate + "T00:00:00Z");
  nextDate.setUTCDate(nextDate.getUTCDate() + 1);
  const nextDateStr = nextDate.toISOString().slice(0, 10);

  // Compute costs from api_request_log + billing_rates
  const details = await computeBillingForPeriod(ctx.db, null, targetDate, nextDateStr);

  if (details.length === 0) {
    return json({ date: targetDate, dryRun, message: "No requests to aggregate", groups: 0, totalCostUsd: 0 });
  }

  const totalCost = details.reduce((sum, d) => sum + d.costUsd, 0);

  // Also aggregate LLM usage costs
  const llmRows = await ctx.db.select().from(llmUsageDaily).where(eq(llmUsageDaily.date, targetDate));
  let llmTotalCost = 0;
  for (const row of llmRows) llmTotalCost += parseFloat(row.costUsd);

  // Insert debit entries for each key's daily total
  if (!dryRun) {
    const keyTotals = new Map<string, number>();
    for (const d of details) {
      keyTotals.set(d.keyPrefix, (keyTotals.get(d.keyPrefix) ?? 0) + d.costUsd);
    }
    for (const row of llmRows) {
      const cost = parseFloat(row.costUsd);
      if (cost > 0) keyTotals.set(row.keyPrefix, (keyTotals.get(row.keyPrefix) ?? 0) + cost);
    }

    const month = targetDate.slice(0, 7);
    for (const [keyPrefix, total] of keyTotals) {
      if (total <= 0) continue;
      const existing = await ctx.db.select().from(billingLedger).where(and(
        eq(billingLedger.keyPrefix, keyPrefix),
        eq(billingLedger.entryType, "debit"),
        eq(billingLedger.referenceMonth, month),
        eq(billingLedger.description, `Usage for ${targetDate}`),
      ));
      if (existing.length === 0) {
        await ctx.db.insert(billingLedger).values({
          keyPrefix, entryType: "debit", amountUsd: total.toFixed(6),
          description: `Usage for ${targetDate}`, referenceMonth: month, actor: auth.userEmail,
        });
      }
    }
  }

  return json({
    date: targetDate, dryRun, requestsProcessed: details.reduce((sum, d) => sum + d.requests, 0),
    groups: details.length, totalCostUsd: Math.round(totalCost * 1_000_000) / 1_000_000,
    llmCostUsd: Math.round(llmTotalCost * 1_000_000) / 1_000_000,
    details,
  });
}, true, "billing:write");

// Issue a billing credit
route("POST", "/v1/billing/credit", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const body = await req.json() as {
    key_prefix: string;
    amount_usd: number;
    description?: string;
  };

  if (!body.key_prefix || !body.amount_usd) {
    return json({ error: "key_prefix and amount_usd are required" }, 400);
  }
  if (body.amount_usd <= 0) {
    return json({ error: "amount_usd must be positive" }, 400);
  }

  // Verify the target key exists
  const targetKey = await getApiKeyByPrefix(ctx.db, body.key_prefix);
  if (!targetKey) {
    return json({ error: "Key not found" }, 404);
  }

  const entry = await ctx.db.insert(billingLedger).values({
    keyPrefix: body.key_prefix,
    entryType: "credit",
    amountUsd: String(body.amount_usd),
    description: body.description ?? "Manual credit",
    actor: auth.userEmail,
  }).returning();

  return json({ credited: true, entry: entry[0] });
}, true, "billing:write");

// View billing ledger for a key
route("GET", "/v1/billing/ledger", async (req, ctx) => {
  const url = new URL(req.url);
  const keyPrefix = url.searchParams.get("key_prefix");
  if (!keyPrefix) {
    return json({ error: "key_prefix query parameter is required" }, 400);
  }

  const entries = await ctx.db
    .select()
    .from(billingLedger)
    .where(eq(billingLedger.keyPrefix, keyPrefix));

  return json({ keyPrefix, entries });
}, true, "billing:read");

// View billing balance for a key
route("GET", "/v1/billing/balance", async (req, ctx) => {
  const url = new URL(req.url);
  const keyPrefix = url.searchParams.get("key_prefix");
  if (!keyPrefix) {
    return json({ error: "key_prefix query parameter is required" }, 400);
  }

  const entries = await ctx.db
    .select()
    .from(billingLedger)
    .where(eq(billingLedger.keyPrefix, keyPrefix));

  let credits = 0;
  let debits = 0;
  for (const e of entries) {
    const amt = parseFloat(e.amountUsd);
    if (e.entryType === "credit") credits += amt;
    else debits += amt;
  }

  return json({
    keyPrefix,
    credits: Math.round(credits * 1_000_000) / 1_000_000,
    debits: Math.round(debits * 1_000_000) / 1_000_000,
    balance: Math.round((credits - debits) * 1_000_000) / 1_000_000,
  });
}, true, "billing:read");

// View billing rates
route("GET", "/v1/billing/rates", async (_req, ctx) => {
  const rates = await ctx.db
    .select()
    .from(billingRates)
    .where(isNull(billingRates.effectiveTo));

  return json({ rates });
}, true, "billing:read");

// Data download endpoint - generate signed URLs for parquet files
route("GET", "/v1/data/download", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const url = new URL(req.url);

  const feed = url.searchParams.get("feed");
  const from = url.searchParams.get("from");
  const to = url.searchParams.get("to");
  const msgType = url.searchParams.get("type") ?? undefined;
  const expiresParam = url.searchParams.get("expires") ?? "12h";

  // Validate required params
  if (!feed || !from || !to) {
    return json({ error: "feed, from, and to query parameters are required" }, 400);
  }

  // Validate feed name
  if (!FEED_CONFIG[feed]) {
    return json({ error: `Invalid feed: ${feed}. Valid feeds: ${Object.keys(FEED_CONFIG).join(", ")}` }, 400);
  }

  // Validate date format
  const dateRegex = /^\d{4}-\d{2}-\d{2}$/;
  if (!dateRegex.test(from) || !dateRegex.test(to)) {
    return json({ error: "from and to must be YYYY-MM-DD format" }, 400);
  }

  // Validate date range (max 7 days)
  const fromDate = new Date(from);
  const toDate = new Date(to);
  const daysDiff = (toDate.getTime() - fromDate.getTime()) / (1000 * 60 * 60 * 24);
  if (daysDiff < 0) {
    return json({ error: "from must be before or equal to to" }, 400);
  }
  if (daysDiff > 6) {
    return json({ error: "Maximum date range is 7 days" }, 400);
  }

  // Enforce key feed restrictions
  if (!auth.allowedFeeds.includes(feed)) {
    return json({ error: `Key not authorized for feed: ${feed}` }, 403);
  }

  // Enforce key date range restrictions (clamp to allowed range)
  const keyStart = new Date(auth.dateRangeStart);
  const keyEnd = new Date(auth.dateRangeEnd);

  // Check for any overlap
  if (toDate < keyStart || fromDate > keyEnd) {
    return json({
      error: `Key date range is ${auth.dateRangeStart} to ${auth.dateRangeEnd}. Requested range has no overlap.`,
    }, 403);
  }

  // Clamp to allowed range
  const clampedFrom = fromDate < keyStart ? keyStart : fromDate;
  const clampedTo = toDate > keyEnd ? keyEnd : toDate;
  const effectiveFrom = clampedFrom.toISOString().slice(0, 10);
  const effectiveTo = clampedTo.toISOString().slice(0, 10);

  // Parse expires param (e.g., "12h", "6h")
  const expiresMatch = expiresParam.match(/^(\d+)h$/);
  if (!expiresMatch) {
    return json({ error: "expires must be in format like '12h'" }, 400);
  }
  const expiresInHours = parseInt(expiresMatch[1], 10);
  if (expiresInHours < 1 || expiresInHours > 12) {
    return json({ error: "expires must be between 1h and 12h" }, 400);
  }

  const bucket = Deno.env.get("GCS_BUCKET");
  if (!bucket) {
    return json({ error: "GCS_BUCKET not configured" }, 503);
  }

  // List and sign files
  const files = await listParquetFiles(bucket, feed, effectiveFrom, effectiveTo, msgType);

  if (files.length > 200) {
    return json({ error: `Too many files (${files.length}). Maximum 200 per request. Narrow your date range or filter by type.` }, 400);
  }

  const signedFiles = await generateSignedUrls(bucket, files, expiresInHours);

  // Audit log (fire-and-forget)
  logDataAccess(ctx.db, {
    keyPrefix: auth.keyPrefix,
    userEmail: auth.userEmail,
    feed,
    dateFrom: effectiveFrom,
    dateTo: effectiveTo,
    msgType: msgType ?? null,
    filesCount: signedFiles.length,
  }).catch((err) => console.error("Failed to log data access:", err));

  return json({
    feed,
    from: effectiveFrom,
    to: effectiveTo,
    type: msgType ?? null,
    files: signedFiles,
    expiresIn: `${expiresInHours}h`,
  });
}, true, "datasets:read", "public");

// Data feeds listing endpoint — enriched from catalog when available
route("GET", "/v1/data/feeds", async () => {
  const bucket = Deno.env.get("GCS_BUCKET");
  if (bucket) {
    try {
      const catalog = await getCatalog(bucket);
      if (catalog) {
        const feeds = catalog.feeds.map((f) => ({
          name: f.feed,
          prefix: f.prefix,
          stream: f.stream,
          messageTypes: f.message_types,
          dateMin: f.date_min,
          dateMax: f.date_max,
          totalFiles: f.total_files,
          totalRows: f.total_rows,
        }));
        return json({ feeds, catalogGeneratedAt: catalog.generated_at });
      }
    } catch {
      // Fall through to FEED_CONFIG
    }
  }
  const feeds = Object.entries(FEED_CONFIG).map(([name, info]) => ({
    name,
    prefix: info.prefix,
    stream: info.stream,
    messageTypes: info.messageTypes,
  }));
  return json({ feeds });
}, true, "datasets:read", "public");

// Data catalog endpoint — discover available feeds and dates
route("GET", "/v1/data/catalog", async (req) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const url = new URL(req.url);

  const feedParam = url.searchParams.get("feed");
  const fromParam = url.searchParams.get("from");
  const toParam = url.searchParams.get("to");

  const bucket = Deno.env.get("GCS_BUCKET");
  if (!bucket) {
    return json({ error: "GCS_BUCKET not configured" }, 503);
  }

  const catalog = await getCatalog(bucket);
  if (!catalog) {
    return json({ error: "Catalog not available. Run parquet-gen catalog to generate." }, 503);
  }

  // Filter feeds by key's allowed feeds
  const allowedFeeds = catalog.feeds.filter((f) =>
    auth.allowedFeeds.includes(f.feed)
  );

  if (feedParam) {
    // Single-feed detail: return dates within range
    const feedSummary = allowedFeeds.find((f) => f.feed === feedParam);
    if (!feedSummary) {
      return json({ error: `Feed not found or not authorized: ${feedParam}` }, 404);
    }

    let dates = feedSummary.dates;
    if (fromParam) {
      dates = dates.filter((d) => d >= fromParam);
    }
    if (toParam) {
      dates = dates.filter((d) => d <= toParam);
    }

    // Build per-date info from totals (lightweight, no per-date manifest fetch)
    const dateEntries = dates.map((d) => ({
      date: d,
      messageTypes: feedSummary.message_types,
    }));

    return json({
      feed: feedParam,
      from: fromParam ?? feedSummary.date_min,
      to: toParam ?? feedSummary.date_max,
      dates: dateEntries,
    });
  }

  // Overview: all authorized feeds
  const feedOverviews = allowedFeeds.map((f) => ({
    feed: f.feed,
    stream: f.stream,
    messageTypes: f.message_types,
    dateMin: f.date_min,
    dateMax: f.date_max,
    totalFiles: f.total_files,
    totalRows: f.total_rows,
  }));

  return json({
    feeds: feedOverviews,
    catalogGeneratedAt: catalog.generated_at,
  });
}, true, "datasets:read", "public");

// Data schemas endpoint — discover parquet column schemas
route("GET", "/v1/data/schemas", async (req) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const url = new URL(req.url);

  const feedParam = url.searchParams.get("feed");
  const typeParam = url.searchParams.get("type");

  const bucket = Deno.env.get("GCS_BUCKET");
  if (!bucket) {
    return json({ error: "GCS_BUCKET not configured" }, 503);
  }

  const catalog = await getCatalog(bucket);
  if (!catalog) {
    return json({ error: "Catalog not available. Run parquet-gen catalog to generate." }, 503);
  }

  // Filter feeds by allowed feeds
  let feeds = catalog.feeds.filter((f) => auth.allowedFeeds.includes(f.feed));
  if (feedParam) {
    feeds = feeds.filter((f) => f.feed === feedParam);
  }

  // deno-lint-ignore no-explicit-any
  const schemas: any[] = [];
  for (const f of feeds) {
    for (const [msgType, schemaInfo] of Object.entries(f.schemas)) {
      if (typeParam && msgType !== typeParam) continue;
      schemas.push({
        feed: f.feed,
        messageType: msgType,
        schemaName: schemaInfo.schema_name,
        schemaVersion: schemaInfo.schema_version,
        columns: schemaInfo.columns.map((c) => ({
          name: c.name,
          type: c.arrow_type,
          nullable: c.nullable,
        })),
      });
    }
  }

  return json({ schemas });
}, true, "datasets:read", "public");

// DuckDB parquet query endpoints
route("GET", "/v1/data/trades", async (req) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const url = new URL(req.url);

  const feed = url.searchParams.get("feed");
  if (!feed || !VALID_DATA_FEEDS.includes(feed)) {
    return json({ error: `Invalid or missing feed. Valid: ${VALID_DATA_FEEDS.join(", ")}` }, 400);
  }

  if (!auth.allowedFeeds.includes(feed)) {
    return json({ error: `Key not authorized for feed: ${feed}` }, 403);
  }

  const date = url.searchParams.get("date") ?? new Date().toISOString().slice(0, 10);
  const dateRegex = /^\d{4}-\d{2}-\d{2}$/;
  if (!dateRegex.test(date)) {
    return json({ error: "date must be YYYY-MM-DD format" }, 400);
  }

  // Check date range
  if (date < auth.dateRangeStart || date > auth.dateRangeEnd) {
    return json({ error: `Date ${date} outside key range ${auth.dateRangeStart} to ${auth.dateRangeEnd}` }, 403);
  }

  const limitParam = url.searchParams.get("limit");
  const limit = Math.min(Math.max(parseInt(limitParam ?? "20", 10) || 20, 1), 1000);

  const bucket = Deno.env.get("GCS_BUCKET");
  if (!bucket) return json({ error: "GCS_BUCKET not configured" }, 503);

  try {
    const sql = buildTradeSQL(bucket, feed, date, limit);
    const result = await duckdbQuery(sql);
    return json({ feed, date, count: result.rows.length, trades: result.rows });
  } catch (err) {
    console.error("Trade query failed:", err);
    return json({ error: `Query failed: ${(err as Error).message}` }, 500);
  }
}, true, "datasets:read", "public");

route("GET", "/v1/data/prices", async (req) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const url = new URL(req.url);

  const feed = url.searchParams.get("feed");
  if (!feed || !VALID_DATA_FEEDS.includes(feed)) {
    return json({ error: `Invalid or missing feed. Valid: ${VALID_DATA_FEEDS.join(", ")}` }, 400);
  }

  if (!auth.allowedFeeds.includes(feed)) {
    return json({ error: `Key not authorized for feed: ${feed}` }, 403);
  }

  const date = url.searchParams.get("date") ?? new Date().toISOString().slice(0, 10);
  const dateRegex = /^\d{4}-\d{2}-\d{2}$/;
  if (!dateRegex.test(date)) {
    return json({ error: "date must be YYYY-MM-DD format" }, 400);
  }

  if (date < auth.dateRangeStart || date > auth.dateRangeEnd) {
    return json({ error: `Date ${date} outside key range ${auth.dateRangeStart} to ${auth.dateRangeEnd}` }, 403);
  }

  const hour = url.searchParams.get("hour") ?? undefined;
  if (hour && !/^\d{4}$/.test(hour)) {
    return json({ error: "hour must be HHMM format (e.g., 1400)" }, 400);
  }

  const bucket = Deno.env.get("GCS_BUCKET");
  if (!bucket) return json({ error: "GCS_BUCKET not configured" }, 503);

  try {
    const sql = buildPriceSQL(bucket, feed, date, hour);
    const result = await duckdbQuery(sql);
    return json({ feed, date, hour: hour ?? null, count: result.rows.length, prices: result.rows });
  } catch (err) {
    console.error("Price query failed:", err);
    return json({ error: `Query failed: ${(err as Error).message}` }, 500);
  }
}, true, "datasets:read", "public");

// Event volume aggregation endpoint
route("GET", "/v1/data/events", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const url = new URL(req.url);

  const feed = url.searchParams.get("feed");
  if (!feed || !VALID_DATA_FEEDS.includes(feed)) {
    return json({ error: `Invalid or missing feed. Valid: ${VALID_DATA_FEEDS.join(", ")}` }, 400);
  }

  if (!auth.allowedFeeds.includes(feed)) {
    return json({ error: `Key not authorized for feed: ${feed}` }, 403);
  }

  const date = url.searchParams.get("date") ?? new Date().toISOString().slice(0, 10);
  const dateRegex = /^\d{4}-\d{2}-\d{2}$/;
  if (!dateRegex.test(date)) {
    return json({ error: "date must be YYYY-MM-DD format" }, 400);
  }

  if (date < auth.dateRangeStart || date > auth.dateRangeEnd) {
    return json({ error: `Date ${date} outside key range ${auth.dateRangeStart} to ${auth.dateRangeEnd}` }, 403);
  }

  const limitParam = url.searchParams.get("limit");
  const limit = Math.min(Math.max(parseInt(limitParam ?? "20", 10) || 20, 1), 100);

  const bucket = Deno.env.get("GCS_BUCKET");
  if (!bucket) return json({ error: "GCS_BUCKET not configured" }, 503);

  try {
    // Step 1: Query DuckDB for event-level volume
    const eventSQL = buildEventVolumeSQL(bucket, feed, date, limit);
    const eventResult = await duckdbQuery(eventSQL);

    if (eventResult.rows.length === 0) {
      return json({ feed, date, volumeUnit: VOLUME_UNITS[feed], events: [] });
    }

    const eventIds = eventResult.rows.map((r) => String(r.event_id));

    // Step 2: Get top markets per event
    const marketsSQL = buildEventMarketsSQL(bucket, feed, date, eventIds, 5);
    const marketsResult = await duckdbQuery(marketsSQL);

    // Group top markets by event_id
    const marketsByEvent: Record<string, Record<string, unknown>[]> = {};
    for (const row of marketsResult.rows) {
      const eid = String(row.event_id);
      if (!marketsByEvent[eid]) marketsByEvent[eid] = [];
      marketsByEvent[eid].push({
        ticker: row.ticker,
        tradeCount: row.trade_count,
        volume: row.volume,
      });
    }

    // Step 3: Batch-enrich event IDs with secmaster metadata
    const metadata: Record<string, Record<string, unknown>> = {};

    if (feed === "kalshi") {
      const rows = await ctx.db
        .select({
          eventTicker: events.eventTicker,
          title: events.title,
          category: events.category,
          status: events.status,
          strikeDate: events.strikeDate,
        })
        .from(events)
        .where(and(inArray(events.eventTicker, eventIds), isNull(events.deletedAt)));
      for (const row of rows) {
        metadata[row.eventTicker] = {
          title: row.title,
          category: row.category,
          status: row.status,
          strikeDate: row.strikeDate?.toISOString() ?? null,
        };
      }
    } else if (feed === "polymarket") {
      const rows = await ctx.db
        .select({
          conditionId: polymarketConditions.conditionId,
          question: polymarketConditions.question,
          status: polymarketConditions.status,
          endDate: polymarketConditions.endDate,
        })
        .from(polymarketConditions)
        .where(and(inArray(polymarketConditions.conditionId, eventIds), isNull(polymarketConditions.deletedAt)));
      for (const row of rows) {
        metadata[row.conditionId] = {
          question: row.question,
          status: row.status,
          endDate: row.endDate?.toISOString() ?? null,
        };
      }
    } else if (feed === "kraken-futures") {
      const rows = await ctx.db
        .select({
          pairId: pairs.pairId,
          base: pairs.base,
          quote: pairs.quote,
          status: pairs.status,
        })
        .from(pairs)
        .where(and(inArray(pairs.pairId, eventIds), isNull(pairs.deletedAt)));
      for (const row of rows) {
        metadata[row.pairId] = {
          symbol: `${row.base}/${row.quote}`,
          status: row.status ?? "active",
        };
      }
    }

    // Step 4: Combine results
    const enrichedEvents = eventResult.rows.map((row) => {
      const eid = String(row.event_id);
      return {
        eventId: eid,
        totalTradeCount: row.total_trade_count,
        totalVolume: row.total_volume,
        marketCount: row.market_count ?? 1,
        metadata: metadata[eid] ?? null,
        topMarkets: marketsByEvent[eid] ?? [],
      };
    });

    return json({
      feed,
      date,
      volumeUnit: VOLUME_UNITS[feed],
      count: enrichedEvents.length,
      events: enrichedEvents,
    });
  } catch (err) {
    console.error("Event volume query failed:", err);
    return json({ error: `Query failed: ${(err as Error).message}` }, 500);
  }
}, true, "datasets:read", "public");

// Volume summary endpoint (per-feed or cross-feed)
route("GET", "/v1/data/volume", async (req) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const url = new URL(req.url);

  const feedParam = url.searchParams.get("feed");
  if (feedParam && !VALID_DATA_FEEDS.includes(feedParam)) {
    return json({ error: `Invalid feed. Valid: ${VALID_DATA_FEEDS.join(", ")}` }, 400);
  }

  if (feedParam && !auth.allowedFeeds.includes(feedParam)) {
    return json({ error: `Key not authorized for feed: ${feedParam}` }, 403);
  }

  const date = url.searchParams.get("date") ?? new Date().toISOString().slice(0, 10);
  const dateRegex = /^\d{4}-\d{2}-\d{2}$/;
  if (!dateRegex.test(date)) {
    return json({ error: "date must be YYYY-MM-DD format" }, 400);
  }

  if (date < auth.dateRangeStart || date > auth.dateRangeEnd) {
    return json({ error: `Date ${date} outside key range ${auth.dateRangeStart} to ${auth.dateRangeEnd}` }, 403);
  }

  const bucket = Deno.env.get("GCS_BUCKET");
  if (!bucket) return json({ error: "GCS_BUCKET not configured" }, 503);

  const feeds = feedParam
    ? [feedParam]
    : VALID_DATA_FEEDS.filter((f) => auth.allowedFeeds.includes(f));

  const feedSummaries: Record<string, unknown>[] = [];

  for (const feed of feeds) {
    if (!auth.allowedFeeds.includes(feed)) continue;

    try {
      const totalSQL = buildTotalVolumeSQL(bucket, feed, date);
      const topSQL = buildTopTickersSQL(bucket, feed, date, 5);
      const [totalResult, topResult] = await Promise.all([
        duckdbQuery(totalSQL),
        duckdbQuery(topSQL),
      ]);

      const totals = totalResult.rows[0] ?? { total_trade_count: 0, total_volume: 0, active_tickers: 0 };

      feedSummaries.push({
        feed,
        totalTradeCount: totals.total_trade_count,
        totalVolume: totals.total_volume,
        volumeUnit: VOLUME_UNITS[feed],
        activeTickers: totals.active_tickers,
        topTickers: topResult.rows.map((r) => ({
          ticker: r.ticker,
          tradeCount: r.trade_count,
          volume: r.volume,
        })),
      });
    } catch (err) {
      console.error(`Volume query failed for ${feed}:`, err);
      feedSummaries.push({
        feed,
        error: (err as Error).message,
      });
    }
  }

  return json({ date, feeds: feedSummaries });
}, true, "datasets:read", "public");

route("GET", "/v1/data/freshness", async (req) => {
  const url = new URL(req.url);
  const feedParam = url.searchParams.get("feed");

  const bucket = Deno.env.get("GCS_BUCKET");
  if (!bucket) return json({ error: "GCS_BUCKET not configured" }, 503);

  const feeds = feedParam ? [feedParam] : VALID_DATA_FEEDS;
  const staleThresholdHours = 7;
  const results: Record<string, unknown>[] = [];

  const { Storage } = await import("@google-cloud/storage");
  const storage = new Storage();

  for (const feed of feeds) {
    if (!VALID_DATA_FEEDS.includes(feed)) {
      results.push({ feed, status: "unknown", error: "Invalid feed" });
      continue;
    }

    try {
      const prefix = FEED_PATHS[feed];

      // Use apiResponse to get prefixes (date dirs)
      const [, , apiResp] = await storage.bucket(bucket).getFiles({
        prefix: `${prefix}/`,
        delimiter: "/",
        autoPaginate: false,
      });

      const prefixes: string[] = (apiResp as { prefixes?: string[] }).prefixes ?? [];
      const dates = prefixes
        .map((p: string) => p.replace(`${prefix}/`, "").replace("/", ""))
        .filter((d: string) => /^\d{4}-\d{2}-\d{2}$/.test(d))
        .sort()
        .reverse();

      if (dates.length === 0) {
        results.push({ feed, status: "no_data", newest_date: null, stale: true });
        continue;
      }

      const newestDate = dates[0];

      // List files in newest date directory
      const [dateFiles] = await storage.bucket(bucket).getFiles({
        prefix: `${prefix}/${newestDate}/`,
      });

      let newestHour: string | null = null;
      for (const f of dateFiles) {
        const name = f.name.split("/").pop() ?? "";
        if (!name.endsWith(".parquet") && !name.endsWith(".jsonl") && !name.endsWith(".jsonl.gz")) continue;
        const base = name.replace(".parquet", "").replace(".jsonl.gz", "").replace(".jsonl", "");
        const lastUnderscore = base.lastIndexOf("_");
        if (lastUnderscore === -1) continue;
        const hourPart = base.substring(lastUnderscore + 1);
        if (/^\d{4}$/.test(hourPart)) {
          if (!newestHour || hourPart > newestHour) {
            newestHour = hourPart;
          }
        }
      }

      // Calculate age
      const now = new Date();
      const dateObj = new Date(newestDate + "T00:00:00Z");
      if (newestHour) {
        dateObj.setUTCHours(parseInt(newestHour.slice(0, 2), 10));
      }
      const ageHours = (now.getTime() - dateObj.getTime()) / 3600000;
      const stale = ageHours > staleThresholdHours;

      results.push({
        feed,
        status: stale ? "stale" : "fresh",
        newest_date: newestDate,
        newest_hour: newestHour,
        age_hours: Math.round(ageHours * 10) / 10,
        stale,
      });
    } catch (err) {
      console.error(`Freshness check failed for ${feed}:`, err);
      results.push({ feed, status: "error", error: (err as Error).message });
    }
  }

  return json({
    checked_at: new Date().toISOString(),
    stale_threshold_hours: staleThresholdHours,
    feeds: results,
  });
}, true, "datasets:read", "public");

// Live price snapshots from Redis (populated by ssmd-snap)
route("GET", "/v1/data/snap", async (req) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;
  const url = new URL(req.url);

  const feed = url.searchParams.get("feed");
  if (!feed || !VALID_DATA_FEEDS.includes(feed)) {
    return json({ error: `Invalid or missing feed. Valid: ${VALID_DATA_FEEDS.join(", ")}` }, 400);
  }

  if (!auth.allowedFeeds.includes(feed)) {
    return json({ error: `Key not authorized for feed: ${feed}` }, 403);
  }

  const redis = await getRedis();
  const tickersParam = url.searchParams.get("tickers");

  // deno-lint-ignore no-explicit-any
  let rawEntries: Array<{ key: string; value: string | null }> = [];

  if (tickersParam) {
    // MGET specific tickers
    const tickers = tickersParam.split(",").map((t) => t.trim()).filter(Boolean);
    if (tickers.length === 0) {
      return json({ error: "tickers must not be empty when provided" }, 400);
    }
    if (tickers.length > 500) {
      return json({ error: "Maximum 500 tickers per request" }, 400);
    }

    const keys = tickers.map((t) => `snap:${feed}:${t}`);
    const values = await redis.mget(...keys);
    for (let i = 0; i < keys.length; i++) {
      rawEntries.push({ key: tickers[i], value: values[i] ?? null });
    }
  } else {
    // SCAN for all keys matching this feed (limit 500)
    const pattern = `snap:${feed}:*`;
    const prefix = `snap:${feed}:`;
    let cursor = "0";
    const seen = new Set<string>();

    do {
      const [nextCursor, keys] = await redis.scan(cursor, { pattern, count: 100 });
      cursor = nextCursor;
      for (const key of keys) {
        if (!seen.has(key) && seen.size < 500) {
          seen.add(key);
        }
      }
    } while (cursor !== "0" && seen.size < 500);

    if (seen.size > 0) {
      const keyArray = [...seen];
      const values = await redis.mget(...keyArray);
      for (let i = 0; i < keyArray.length; i++) {
        const ticker = keyArray[i].slice(prefix.length);
        rawEntries.push({ key: ticker, value: values[i] ?? null });
      }
    }
  }

  // Parse JSON values and convert Kalshi prices from cents to dollars
  const isKalshi = feed === "kalshi";
  const priceFields = ["yes_bid", "yes_ask", "no_bid", "no_ask", "last_price"];

  // deno-lint-ignore no-explicit-any
  const snapshots: any[] = [];
  for (const entry of rawEntries) {
    if (!entry.value) continue;
    try {
      const parsed = JSON.parse(entry.value);
      if (isKalshi) {
        for (const field of priceFields) {
          if (typeof parsed[field] === "number") {
            parsed[field] = parsed[field] / 100;
          }
        }
      }
      // Use the ticker from the key, not the payload, for consistency
      parsed._ticker = entry.key;
      snapshots.push(parsed);
    } catch {
      // Skip unparseable entries
    }
  }

  return json({
    feed,
    snapshots,
    count: snapshots.length,
  });
}, true, "datasets:read", "public");

// Monitor hierarchy endpoints (populated by ssmd-cache into Redis)
route("GET", "/v1/monitor/categories", async () => {
  const redis = await getRedis();
  const raw = await redis.hgetall("monitor:categories");
  // hgetall returns flat array: [field1, value1, field2, value2, ...]
  const categories: Array<{ name: string; [key: string]: unknown }> = [];
  for (let i = 0; i < raw.length; i += 2) {
    const name = raw[i];
    try {
      const data = JSON.parse(raw[i + 1]);
      categories.push({ name, ...data });
    } catch {
      // skip unparseable entries
    }
  }
  return json({ categories });
}, true, "datasets:read", "public");

route("GET", "/v1/monitor/series", async (req) => {
  const url = new URL(req.url);
  const category = url.searchParams.get("category");
  if (!category) {
    return json({ error: "category query parameter is required" }, 400);
  }
  const redis = await getRedis();
  const raw = await redis.hgetall(`monitor:series:${category}`);
  const series: Array<{ ticker: string; [key: string]: unknown }> = [];
  for (let i = 0; i < raw.length; i += 2) {
    const ticker = raw[i];
    try {
      const data = JSON.parse(raw[i + 1]);
      series.push({ ticker, ...data });
    } catch {
      // skip unparseable entries
    }
  }
  return json({ series });
}, true, "datasets:read", "public");

route("GET", "/v1/monitor/events", async (req) => {
  const url = new URL(req.url);
  const series = url.searchParams.get("series");
  if (!series) {
    return json({ error: "series query parameter is required" }, 400);
  }
  const redis = await getRedis();
  const raw = await redis.hgetall(`monitor:events:${series}`);
  const events: Array<{ ticker: string; [key: string]: unknown }> = [];
  for (let i = 0; i < raw.length; i += 2) {
    const ticker = raw[i];
    try {
      const data = JSON.parse(raw[i + 1]);
      events.push({ ticker, ...data });
    } catch {
      // skip unparseable entries
    }
  }
  return json({ events });
}, true, "datasets:read", "public");

route("GET", "/v1/monitor/markets", async (req) => {
  const url = new URL(req.url);
  const event = url.searchParams.get("event");
  if (!event) {
    return json({ error: "event query parameter is required" }, 400);
  }
  const redis = await getRedis();
  const raw = await redis.hgetall(`monitor:markets:${event}`);

  // Parse market entries
  const tickers: string[] = [];
  // deno-lint-ignore no-explicit-any
  const marketMap = new Map<string, any>();
  for (let i = 0; i < raw.length; i += 2) {
    const ticker = raw[i];
    try {
      const data = JSON.parse(raw[i + 1]);
      tickers.push(ticker);
      marketMap.set(ticker, { ticker, ...data });
    } catch {
      // skip unparseable entries
    }
  }

  // Merge snap data for live prices (exchange-aware)
  if (tickers.length > 0) {
    // Build snap keys — handle per-exchange format differences
    const nonPmTickers: string[] = [];
    const snapKeys: string[] = [];
    for (const t of tickers) {
      const market = marketMap.get(t);
      const exchange = market?.exchange ?? "kalshi";

      if (exchange === "kraken-futures" || exchange === "kraken") {
        // Monitor stores "kraken:PF_XBTUSD" — strip prefix, use "kraken-futures" feed
        let rawTicker = t;
        while (rawTicker.startsWith("kraken:")) rawTicker = rawTicker.slice(7);
        snapKeys.push(`snap:kraken-futures:${rawTicker}`);
        nonPmTickers.push(t);
      } else if (exchange === "polymarket") {
        // PM tokens handled separately via condition-level lookup below
      } else {
        snapKeys.push(`snap:${exchange}:${t}`);
        nonPmTickers.push(t);
      }
    }

    if (snapKeys.length > 0) {
      const snapValues = await redis.mget(...snapKeys);
      for (let i = 0; i < nonPmTickers.length; i++) {
        const snapRaw = snapValues[i];
        if (!snapRaw) continue;
        try {
          const snap = JSON.parse(snapRaw);
          const market = marketMap.get(nonPmTickers[i]);
          if (!market) continue;
          const exchange = market.exchange ?? "kalshi";

          if (exchange === "kraken-futures" || exchange === "kraken") {
            if (typeof snap.bid === "number") market.bid = snap.bid;
            if (typeof snap.ask === "number") market.ask = snap.ask;
            if (typeof snap.last === "number") market.last = snap.last;
            if (snap.funding_rate != null) market.funding_rate = snap.funding_rate;
          } else {
            // Kalshi: snap data is nested in msg object, convert cents to dollars
            const snapData = snap.msg ?? snap;
            for (const field of ["yes_bid", "yes_ask", "price"]) {
              if (typeof snapData[field] === "number") {
                market[field === "price" ? "last" : field] = snapData[field] / 100;
              }
            }
            if (typeof snapData.volume === "number") market.volume = snapData.volume;
            if (typeof snapData.open_interest === "number") market.open_interest = snapData.open_interest;
          }
        } catch {
          // skip unparseable snap
        }
      }
    }

    // Polymarket: single snap lookup by condition_id (= event ticker from URL)
    const hasPmMarkets = tickers.some((t) => marketMap.get(t)?.exchange === "polymarket");
    if (hasPmMarkets && event) {
      try {
        const conditionSnap = await redis.get(`snap:polymarket:${event}`);
        if (conditionSnap) {
          const snap = JSON.parse(conditionSnap);
          const priceChanges = snap.price_changes ?? [];
          for (const pc of priceChanges) {
            // Match asset_id to token_id (market key)
            const market = marketMap.get(pc.asset_id);
            if (market) {
              market.best_bid = pc.best_bid != null ? Number(pc.best_bid) : null;
              market.best_ask = pc.best_ask != null ? Number(pc.best_ask) : null;
              market.last = pc.price != null ? Number(pc.price) : null;
              if (pc.best_bid != null && pc.best_ask != null) {
                market.spread = Number(pc.best_ask) - Number(pc.best_bid);
              }
            }
          }
        }
      } catch {
        // skip unparseable PM snap
      }
    }
  }

  return json({ markets: [...marketMap.values()] });
}, true, "datasets:read", "public");

// Chat completions proxy (OpenRouter)
route("POST", "/v1/chat/completions", async (req, ctx) => {
  if (!OPENROUTER_API_KEY) {
    return json({ error: "OpenRouter API key not configured" }, 503);
  }

  const auth = (req as Request & { auth: AuthInfo }).auth;

  // Parse request body with error handling
  let body: {
    model: string;
    messages: Array<{ role: string; content: string }>;
    max_tokens?: number;
    [key: string]: unknown;
  };
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON in request body" }, 400);
  }

  // Validate required fields
  if (!body.model || typeof body.model !== "string") {
    return json({ error: "model is required" }, 400);
  }
  if (!Array.isArray(body.messages)) {
    return json({ error: "messages must be an array" }, 400);
  }

  // Check model allowlist
  const modelCheck = checkModelAllowed(body.model);
  if (!modelCheck.allowed) {
    return json({ error: modelCheck.reason }, 403);
  }

  // Apply guardrails
  const settings = await getGuardrailSettings(ctx.db);

  // trivial regex guardrails for poc.
  const guardrailResult = applyGuardrails(body.messages, settings);

  if (!guardrailResult.allowed) {
    return json({ error: guardrailResult.reason }, 403);
  }

  // Use modified messages if PII was redacted
  const messages = guardrailResult.modifiedMessages ?? body.messages;

  // Clamp max_tokens if limit is set
  let maxTokens = body.max_tokens;
  if (settings.maxTokens && (!maxTokens || maxTokens > settings.maxTokens)) {
    maxTokens = settings.maxTokens;
  }

  // Forward to OpenRouter with error handling
  let response: Response;
  try {
    response = await fetch(`${OPENROUTER_BASE_URL}/chat/completions`, {
      method: "POST",
      headers: {
        "Authorization": `Bearer ${OPENROUTER_API_KEY}`,
        "Content-Type": "application/json",
        "HTTP-Referer": "https://ssmd.varshtat.com",
        "X-Title": "ssmd-agent",
      },
      body: JSON.stringify({
        ...body,
        messages,
        max_tokens: maxTokens,
        stream: false, // Force non-streaming - we need JSON response, not SSE chunks
      }),
    });
  } catch (error) {
    console.error("OpenRouter fetch failed:", error);
    return json({ error: "LLM service unavailable" }, 503);
  }

  // Parse response with error handling
  // deno-lint-ignore no-explicit-any
  let data: any;
  try {
    const text = await response.text();
    data = JSON.parse(text);

    // Fix empty arguments: OpenRouter returns "" but OpenAI expects "{}"
    if (data.choices?.[0]?.message?.tool_calls) {
      for (const tc of data.choices[0].message.tool_calls) {
        if (tc.function?.arguments === "") {
          tc.function.arguments = "{}";
        }
      }
    }
  } catch {
    return json({ error: "LLM service returned invalid response" }, 502);
  }

  // Track token usage (best effort - don't fail if tracking fails)
  if (data.usage) {
    try {
      await trackTokenUsage(
        auth.keyPrefix,
        {
          promptTokens: (data.usage as Record<string, number>).prompt_tokens ?? 0,
          completionTokens: (data.usage as Record<string, number>).completion_tokens ?? 0,
        },
        body.model
      );
    } catch (error) {
      console.error("Token usage tracking failed:", error);
      // Continue anyway - don't fail the request
    }
  }

  return new Response(JSON.stringify(data), {
    status: response.status,
    headers: { "Content-Type": "application/json" },
  });
}, true, "llm:chat");

// Helper to create JSON response
function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

// Extract API key from either X-API-Key header or Authorization: Bearer header
function extractApiKey(req: Request): string | null {
  // Check X-API-Key first (our custom header)
  const xApiKey = req.headers.get("X-API-Key");
  if (xApiKey) return xApiKey;

  // Check Authorization header (OpenAI-compatible format)
  const authHeader = req.headers.get("Authorization");
  if (authHeader?.startsWith("Bearer ")) {
    return authHeader.slice(7); // Remove "Bearer " prefix
  }

  return null;
}

// Router function
export function createRouter(ctx: RouteContext): (req: Request) => Promise<Response> {
  // Initialize request log buffer for billing (only if real DB provided)
  let requestLogger: RequestLogBuffer | null = null;
  if (ctx.db && typeof ctx.db.insert === "function") {
    requestLogger = new RequestLogBuffer(ctx.db);
    requestLogger.start();
  }

  return async (req: Request) => {
    const url = new URL(req.url);

    for (const r of routes) {
      if (req.method !== r.method) continue;

      const match = r.pattern.exec(url);
      if (!match) continue;

      // Check auth if required
      let authResult: import("./auth.ts").AuthResult | null = null;
      if (r.requiresAuth) {
        const validate = ctx.authOverride ?? validateApiKey;
        authResult = await validate(
          extractApiKey(req),
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
            keyPrefix: authResult.keyPrefix,
            allowedFeeds: authResult.allowedFeeds,
            dateRangeStart: authResult.dateRangeStart,
            dateRangeEnd: authResult.dateRangeEnd,
            billable: authResult.billable ?? true,
          } as AuthInfo,
        });
      }

      // Add path params to request
      const params = match.pathname.groups;
      Object.defineProperty(req, "params", { value: params });

      const response = await r.handler(req, ctx);

      // Track per-key API usage in Prometheus (GMP scrapes this)
      if (r.requiresAuth && (req as Request & { auth?: AuthInfo }).auth) {
        const auth = (req as Request & { auth: AuthInfo }).auth;
        const path = normalizePath(url.pathname);
        apiRequestsTotal.inc({
          key_prefix: auth.keyPrefix,
          method: req.method,
          path,
          status: String(response.status),
        });

        // Persist billable requests to PostgreSQL for billing
        if (auth.billable && requestLogger) {
          requestLogger.push({
            keyPrefix: auth.keyPrefix,
            method: req.method,
            path,
            statusCode: response.status,
            responseBytes: null,
          });
        }
      }

      // Add rate limit headers to authenticated responses
      if (r.requiresAuth && authResult) {
        const headers = new Headers(response.headers);
        if (authResult.rateLimitRemaining !== undefined) {
          headers.set("X-RateLimit-Remaining", authResult.rateLimitRemaining.toString());
          headers.set("X-RateLimit-Reset", authResult.rateLimitResetAt!.toString());
        }
        return new Response(response.body, { status: response.status, headers });
      }

      return response;
    }

    return json({ error: "Not found" }, 404);
  };
}
