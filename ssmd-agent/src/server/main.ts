// ssmd-data-ts server entry point
import { createServer } from "./mod.ts";
import { initDuckDB, closeDuckDB } from "../lib/duckdb/mod.ts";

const port = parseInt(Deno.env.get("PORT") ?? "8080");
const dataDir = Deno.env.get("DATA_DIR") ?? "/data";
const databaseUrl = Deno.env.get("DATABASE_URL");
const redisUrl = Deno.env.get("REDIS_URL");
// Parse harman database URLs: HARMAN_DATABASE_URLS="name1=postgres://...,name2=postgres://..."
// Falls back to HARMAN_DATABASE_URL as a single pool named "default"
const harmanDatabaseUrls = new Map<string, string>();
const multiUrls = Deno.env.get("HARMAN_DATABASE_URLS");
if (multiUrls) {
  for (const entry of multiUrls.split(",")) {
    const eqIdx = entry.indexOf("=");
    if (eqIdx > 0) {
      harmanDatabaseUrls.set(entry.slice(0, eqIdx).trim(), entry.slice(eqIdx + 1).trim());
    }
  }
} else {
  const singleUrl = Deno.env.get("HARMAN_DATABASE_URL");
  if (singleUrl) harmanDatabaseUrls.set("default", singleUrl);
}

if (!databaseUrl) {
  console.error("DATABASE_URL environment variable is required");
  Deno.exit(1);
}

if (!redisUrl) {
  console.error("REDIS_URL environment variable is required");
  Deno.exit(1);
}

// Initialize DuckDB for parquet queries (non-fatal if it fails)
await initDuckDB().catch((err) => {
  console.error("DuckDB init failed (queries will be unavailable):", err.message);
});

const server = createServer({ port, dataDir, databaseUrl, redisUrl, harmanDatabaseUrls });

// Handle shutdown gracefully
Deno.addSignalListener("SIGINT", async () => {
  console.log("\nShutting down...");
  await closeDuckDB();
  server.shutdown();
  Deno.exit(0);
});
