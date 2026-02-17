// HTTP server middleware

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
