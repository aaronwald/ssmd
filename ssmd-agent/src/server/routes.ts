// HTTP server routes
import { listDatasets } from "./handlers/datasets.ts";

export const API_VERSION = "1.0.0";

export interface RouteContext {
  apiKey: string;
  dataDir: string;
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

// Datasets endpoint
route("GET", "/datasets", async (req, ctx) => {
  const url = new URL(req.url);
  const feedFilter = url.searchParams.get("feed") ?? undefined;
  const fromDate = url.searchParams.get("from") ?? undefined;
  const toDate = url.searchParams.get("to") ?? undefined;

  const datasets = await listDatasets(ctx.dataDir, feedFilter, fromDate, toDate);
  return json({ datasets });
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
        const auth = req.headers.get("Authorization");
        if (!auth || auth !== `Bearer ${ctx.apiKey}`) {
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
