// HTTP server middleware

import { httpRequestDuration, httpRequestsTotal, httpInFlight } from "./metrics.ts";

/**
 * Request logger middleware
 */
export function logger(
  handler: (req: Request) => Promise<Response>
): (req: Request) => Promise<Response> {
  return async (req: Request) => {
    const start = Date.now();
    const res = await handler(req);
    const ms = Date.now() - start;
    console.log(`${req.method} ${new URL(req.url).pathname} ${res.status} ${ms}ms`);
    return res;
  };
}

/**
 * API key authentication middleware
 */
export function requireApiKey(
  apiKey: string,
  handler: (req: Request) => Promise<Response>
): (req: Request) => Promise<Response> {
  return async (req: Request) => {
    const auth = req.headers.get("Authorization");
    if (!auth || auth !== `Bearer ${apiKey}`) {
      return new Response(JSON.stringify({ error: "Unauthorized" }), {
        status: 401,
        headers: { "Content-Type": "application/json" },
      });
    }
    return handler(req);
  };
}

const ALLOWED_ORIGINS: Set<string> = new Set(
  (Deno.env.get("CORS_ORIGINS") ?? "").split(",").map((s) => s.trim()).filter(Boolean)
);

const SECURITY_HEADERS: Record<string, string> = {
  "Strict-Transport-Security": "max-age=31536000; includeSubDomains",
  "X-Content-Type-Options": "nosniff",
  "X-Frame-Options": "DENY",
};

/**
 * CORS and security headers middleware.
 * Set CORS_ORIGINS env var (comma-separated) to allow specific origins.
 * Default: no CORS headers (blocks all cross-origin browser requests).
 */
export function cors(
  handler: (req: Request) => Promise<Response>
): (req: Request) => Promise<Response> {
  return async (req: Request) => {
    const origin = req.headers.get("Origin");
    const allowedOrigin = origin && ALLOWED_ORIGINS.has(origin) ? origin : null;

    if (req.method === "OPTIONS") {
      const headers: Record<string, string> = { ...SECURITY_HEADERS };
      if (allowedOrigin) {
        headers["Access-Control-Allow-Origin"] = allowedOrigin;
        headers["Access-Control-Allow-Methods"] = "GET, POST, DELETE, OPTIONS";
        headers["Access-Control-Allow-Headers"] = "Content-Type, Authorization, X-API-Key";
      }
      return new Response(null, { headers });
    }

    const res = await handler(req);
    const headers = new Headers(res.headers);
    for (const [key, value] of Object.entries(SECURITY_HEADERS)) {
      headers.set(key, value);
    }
    if (allowedOrigin) {
      headers.set("Access-Control-Allow-Origin", allowedOrigin);
    }
    return new Response(res.body, { status: res.status, headers });
  };
}

const PARAM_RESOURCES: Record<string, string> = {
  events: ":ticker",
  markets: ":ticker",
  pairs: ":pairId",
  conditions: ":conditionId",
  fees: ":series",
  keys: ":prefix",
};

/**
 * Normalize URL paths to route patterns for metrics labels.
 * Replaces dynamic segments with param placeholders to avoid
 * high-cardinality label values.
 */
export function normalizePath(pathname: string): string {
  const segments = pathname.split("/");
  // Pattern: /v1/<resource>/<id>[/<sub>]
  if (segments.length >= 4 && segments[1] === "v1") {
    const resource = segments[2];
    const param = PARAM_RESOURCES[resource];
    if (param && segments[3] !== "lookup") {
      segments[3] = param;
    }
  }
  return segments.join("/");
}

/**
 * Prometheus metrics middleware.
 * Records request duration, total count, and in-flight gauge.
 */
export function metricsMiddleware(
  handler: (req: Request) => Promise<Response>
): (req: Request) => Promise<Response> {
  return async (req: Request) => {
    const pathname = new URL(req.url).pathname;
    if (pathname === "/metrics") return handler(req);

    const path = normalizePath(pathname);
    httpInFlight.inc();
    const start = performance.now();
    try {
      const res = await handler(req);
      const duration = (performance.now() - start) / 1000;
      const labels = { method: req.method, path, status: String(res.status) };
      httpRequestDuration.observe(labels, duration);
      httpRequestsTotal.inc(labels);
      return res;
    } catch (err) {
      const duration = (performance.now() - start) / 1000;
      const labels = { method: req.method, path, status: "500" };
      httpRequestDuration.observe(labels, duration);
      httpRequestsTotal.inc(labels);
      throw err;
    } finally {
      httpInFlight.dec();
    }
  };
}
