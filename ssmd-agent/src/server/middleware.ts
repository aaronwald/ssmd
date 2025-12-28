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

/**
 * CORS middleware for development
 */
export function cors(
  handler: (req: Request) => Promise<Response>
): (req: Request) => Promise<Response> {
  return async (req: Request) => {
    if (req.method === "OPTIONS") {
      return new Response(null, {
        headers: {
          "Access-Control-Allow-Origin": "*",
          "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
          "Access-Control-Allow-Headers": "Content-Type, Authorization",
        },
      });
    }

    const res = await handler(req);
    const headers = new Headers(res.headers);
    headers.set("Access-Control-Allow-Origin", "*");
    return new Response(res.body, {
      status: res.status,
      headers,
    });
  };
}
