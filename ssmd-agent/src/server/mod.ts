// Server module exports
export { logger, cors, metricsMiddleware } from "./middleware.ts";
export { createRouter, createFilteredRouter, API_VERSION, type RouteContext, type AuthInfo, type ApiSurface } from "./routes.ts";
export { validateApiKey, hasScope } from "./auth.ts";
export { initDuckDB, closeDuckDB } from "../lib/duckdb/mod.ts";

import { createRouter, createFilteredRouter, type RouteContext } from "./routes.ts";
import { logger, cors, metricsMiddleware } from "./middleware.ts";
import { drizzle } from "drizzle-orm/postgres-js";
import postgres from "postgres";
import * as schema from "../lib/db/schema.ts";

export interface ServerOptions {
  port: number;
  dataDir: string;
  databaseUrl: string;
  redisUrl?: string;  // Optional, uses REDIS_URL env var if not provided
  harmanDatabaseUrls?: Map<string, string>;  // Optional, name→url for harman admin queries
  internalPort?: number;  // Optional, enables dual listener (public + internal)
}

export interface ServerHandle {
  public: Deno.HttpServer<Deno.NetAddr>;
  internal?: Deno.HttpServer<Deno.NetAddr>;
  shutdown(): void;
}

/**
 * Create and start the HTTP server(s).
 * If internalPort is set, creates dual listeners:
 *   - public port: only "public" surface routes + health/version/metrics
 *   - internal port: all routes (no surface filtering)
 * If internalPort is not set, creates a single listener with all routes (backwards compatible).
 */
export function createServer(options: ServerOptions): ServerHandle {
  // Set REDIS_URL if provided via options
  if (options.redisUrl) {
    Deno.env.set("REDIS_URL", options.redisUrl);
  }

  const sql = postgres(options.databaseUrl);
  const db = drizzle(sql, { schema });

  // Create pools for harman databases (admin routes)
  const harmanPools = new Map<string, ReturnType<typeof postgres>>();
  if (options.harmanDatabaseUrls) {
    for (const [name, url] of options.harmanDatabaseUrls) {
      harmanPools.set(name, postgres(url, { max: 5, idle_timeout: 30, connect_timeout: 10 }));
    }
  }

  if (harmanPools.size > 0) {
    console.log(`Harman database connections configured: ${[...harmanPools.keys()].join(", ")}`);
  }

  const ctx: RouteContext = {
    dataDir: options.dataDir,
    db,
    harmanPools,
  };

  if (options.internalPort) {
    // Dual listener mode: public surface on main port, all routes on internal port
    const publicRouter = createFilteredRouter(ctx, "public");
    const publicHandler = cors(metricsMiddleware(logger(publicRouter)));

    const internalRouter = createRouter(ctx);
    const internalHandler = cors(metricsMiddleware(logger(internalRouter)));

    console.log(`ssmd-data-ts public listener on http://localhost:${options.port}`);
    console.log(`ssmd-data-ts internal listener on http://localhost:${options.internalPort}`);

    const publicServer = Deno.serve(
      { port: options.port },
      publicHandler
    );

    const internalServer = Deno.serve(
      { port: options.internalPort },
      internalHandler
    );

    return {
      public: publicServer,
      internal: internalServer,
      shutdown() {
        publicServer.shutdown();
        internalServer.shutdown();
      },
    };
  }

  // Single listener mode (backwards compatible): all routes on main port
  const router = createRouter(ctx);
  const handler = cors(metricsMiddleware(logger(router)));

  console.log(`ssmd-data-ts listening on http://localhost:${options.port}`);

  const server = Deno.serve(
    { port: options.port },
    handler
  );

  return {
    public: server,
    shutdown() {
      server.shutdown();
    },
  };
}
