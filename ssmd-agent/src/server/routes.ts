// HTTP server routes
import { listDatasets } from "./handlers/datasets.ts";
import { globalRegistry } from "./metrics.ts";
import { validateApiKey, hasScope } from "./auth.ts";
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
  createApiKey,
  listApiKeysByUser,
  listAllApiKeys,
  revokeApiKey,
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
  type Database,
} from "../lib/db/mod.ts";
import { generateApiKey, invalidateKeyCache } from "../lib/auth/mod.ts";
import { getUsageForPrefix, getTokenUsage, trackTokenUsage } from "../lib/auth/ratelimit.ts";
import { getGuardrailSettings, applyGuardrails, checkModelAllowed } from "../lib/guardrails/mod.ts";
import { getRedis } from "../lib/redis/mod.ts";
import { listParquetFiles, generateSignedUrls, FEED_CONFIG } from "../lib/gcs/mod.ts";
import { logDataAccess } from "../lib/db/mod.ts";

const USAGE_CACHE_KEY = "cache:keys:usage";
const USAGE_CACHE_TTL = 120; // 2 minutes

export const API_VERSION = "1.0.0";

const OPENROUTER_API_KEY = Deno.env.get("OPENROUTER_API_KEY") ?? "";
const OPENROUTER_BASE_URL = "https://openrouter.ai/api/v1";

export interface RouteContext {
  dataDir: string;
  db: Database;
}

export interface AuthInfo {
  userId: string;
  userEmail: string;
  scopes: string[];
  keyPrefix: string;
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
    asOf: url.searchParams.get("as_of") ?? undefined,
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
}, true, "secmaster:read");

route("GET", "/v1/markets/:ticker", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const market = await getMarket(ctx.db, params.ticker);
  if (!market) {
    return json({ error: "Market not found" }, 404);
  }
  return json(market);
}, true, "secmaster:read");

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
}, true, "secmaster:read");

// Market activity timeseries (added/closed per day)
route("GET", "/v1/secmaster/markets/timeseries", async (req, ctx) => {
  const url = new URL(req.url);
  const days = url.searchParams.get("days")
    ? parseInt(url.searchParams.get("days")!)
    : 30;
  const timeseries = await getMarketTimeseries(ctx.db, days);
  return json({ timeseries });
}, true, "secmaster:read");

// Active markets by category over time
route("GET", "/v1/secmaster/markets/active-by-category", async (req, ctx) => {
  const url = new URL(req.url);
  const days = url.searchParams.get("days")
    ? parseInt(url.searchParams.get("days")!)
    : 7;
  const timeseries = await getActiveMarketsByCategoryTimeseries(ctx.db, days);
  return json({ timeseries });
}, true, "secmaster:read");

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
}, true, "secmaster:read");

route("GET", "/v1/series/stats", async (_req, _ctx) => {
  const stats = await getSeriesStats();
  return json({ stats });
}, true, "secmaster:read");

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
}, true, "secmaster:read");

route("GET", "/v1/pairs/stats", async (_req, ctx) => {
  const stats = await getPairStats(ctx.db);
  return json({ stats });
}, true, "secmaster:read");

route("GET", "/v1/pairs/:pairId/snapshots", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const url = new URL(req.url);
  const snapshots = await getPairSnapshots(ctx.db, params.pairId, {
    from: url.searchParams.get("from") ?? undefined,
    to: url.searchParams.get("to") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ snapshots });
}, true, "secmaster:read");

route("GET", "/v1/pairs/:pairId", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const pair = await getPair(ctx.db, params.pairId);
  if (!pair) {
    return json({ error: "Pair not found" }, 404);
  }
  return json(pair);
}, true, "secmaster:read");

// Conditions endpoints (Polymarket)
route("GET", "/v1/conditions", async (req, ctx) => {
  const url = new URL(req.url);
  const conditions = await listConditions(ctx.db, {
    category: url.searchParams.get("category") ?? undefined,
    status: url.searchParams.get("status") ?? undefined,
    limit: url.searchParams.get("limit") ? parseInt(url.searchParams.get("limit")!) : undefined,
  });
  return json({ conditions });
}, true, "secmaster:read");

route("GET", "/v1/conditions/:conditionId", async (req, ctx) => {
  const params = (req as Request & { params: Record<string, string> }).params;
  const result = await getCondition(ctx.db, params.conditionId);
  if (!result) {
    return json({ error: "Condition not found" }, 404);
  }
  return json(result);
}, true, "secmaster:read");

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
}, true, "secmaster:read");

route("GET", "/v1/health/sla", async (req, ctx) => {
  const url = new URL(req.url);
  const windowDays = url.searchParams.get("window_days")
    ? parseInt(url.searchParams.get("window_days")!)
    : undefined;
  const metrics = await getSlaMetrics(ctx.db, { windowDays });
  return json({ metrics });
}, true, "secmaster:read");

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
}, true, "secmaster:read");

// Key management endpoints
const VALID_SCOPES = [
  "secmaster:read", "datasets:read", "signals:read", "signals:write",
  "admin:read", "admin:write", "llm:chat",
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
  }, 201);
}, true, "admin:write");

route("GET", "/v1/keys", async (req, ctx) => {
  const auth = (req as Request & { auth: AuthInfo }).auth;

  let keys;
  if (hasScope(auth.scopes, "admin:read")) {
    keys = await listAllApiKeys(ctx.db);
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
    })),
  });
}, true, "secmaster:read");

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

  const revoked = await revokeApiKey(ctx.db, params.prefix);
  if (revoked) {
    await invalidateKeyCache(params.prefix);
  }

  return json({ revoked });
}, true, "secmaster:read");

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
  const files = await listParquetFiles(bucket, feed, from, to, msgType);

  if (files.length > 200) {
    return json({ error: `Too many files (${files.length}). Maximum 200 per request. Narrow your date range or filter by type.` }, 400);
  }

  const signedFiles = await generateSignedUrls(bucket, files, expiresInHours);

  // Audit log (fire-and-forget)
  logDataAccess(ctx.db, {
    keyPrefix: auth.keyPrefix,
    userEmail: auth.userEmail,
    feed,
    dateFrom: from,
    dateTo: to,
    msgType: msgType ?? null,
    filesCount: signedFiles.length,
  }).catch((err) => console.error("Failed to log data access:", err));

  return json({
    feed,
    from,
    to,
    type: msgType ?? null,
    files: signedFiles,
    expiresIn: `${expiresInHours}h`,
  });
}, true, "datasets:read");

// Data feeds listing endpoint
route("GET", "/v1/data/feeds", async () => {
  const feeds = Object.entries(FEED_CONFIG).map(([name, info]) => ({
    name,
    prefix: info.prefix,
    stream: info.stream,
    messageTypes: info.messageTypes,
  }));
  return json({ feeds });
}, true, "datasets:read");

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
  return async (req: Request) => {
    const url = new URL(req.url);

    for (const r of routes) {
      if (req.method !== r.method) continue;

      const match = r.pattern.exec(url);
      if (!match) continue;

      // Check auth if required
      if (r.requiresAuth) {
        const authResult = await validateApiKey(
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
