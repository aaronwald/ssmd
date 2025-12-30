// Server module exports
export { logger, requireApiKey, cors } from "./middleware.ts";
export { createRouter, API_VERSION, type RouteContext } from "./routes.ts";

import { createRouter, type RouteContext } from "./routes.ts";
import { logger, cors } from "./middleware.ts";
import { drizzle } from "drizzle-orm/postgres-js";
import postgres from "postgres";
import * as schema from "../lib/db/schema.ts";

export interface ServerOptions {
  port: number;
  apiKey: string;
  dataDir: string;
  databaseUrl: string;
}

/**
 * Create and start the HTTP server
 */
export function createServer(options: ServerOptions): Deno.HttpServer<Deno.NetAddr> {
  const sql = postgres(options.databaseUrl);
  const db = drizzle(sql, { schema });

  const ctx: RouteContext = {
    apiKey: options.apiKey,
    dataDir: options.dataDir,
    db,
  };

  const router = createRouter(ctx);
  const handler = cors(logger(router));

  console.log(`ssmd-data-ts listening on http://localhost:${options.port}`);

  return Deno.serve(
    { port: options.port },
    handler
  );
}
