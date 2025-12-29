// ssmd-data-ts server entry point
import { createServer } from "./mod.ts";

const port = parseInt(Deno.env.get("PORT") ?? "8080");
const apiKey = Deno.env.get("API_KEY");
const dataDir = Deno.env.get("DATA_DIR") ?? "/data";
const databaseUrl = Deno.env.get("DATABASE_URL");

if (!apiKey) {
  console.error("API_KEY environment variable is required");
  Deno.exit(1);
}

if (!databaseUrl) {
  console.error("DATABASE_URL environment variable is required");
  Deno.exit(1);
}

const server = createServer({ port, apiKey, dataDir, databaseUrl });

// Handle shutdown gracefully
Deno.addSignalListener("SIGINT", () => {
  console.log("\nShutting down...");
  server.shutdown();
  Deno.exit(0);
});
