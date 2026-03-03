// Server module exports
export { logger, cors, metricsMiddleware } from "./middleware.ts";
export { createRouter, API_VERSION, type RouteContext, type AuthInfo } from "./routes.ts";
export { validateApiKey, hasScope } from "./auth.ts";
export { initDuckDB, closeDuckDB } from "../lib/duckdb/mod.ts";

import { createRouter, type RouteContext } from "./routes.ts";
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
}

/**
 * Create and start the HTTP server
 */
export function createServer(options: ServerOptions): Deno.HttpServer<Deno.NetAddr> {
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

  const router = createRouter(ctx);
  const handler = cors(metricsMiddleware(logger(router)));

  console.log(`ssmd-data-ts listening on http://localhost:${options.port}`);

  return Deno.serve(
    { port: options.port },
    handler
  );
}
