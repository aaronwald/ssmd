// ssmd-data-ts server entry point
import { createServer } from "./mod.ts";

const port = parseInt(Deno.env.get("PORT") ?? "8080");
const apiKey = Deno.env.get("API_KEY");

if (!apiKey) {
  console.error("API_KEY environment variable is required");
  Deno.exit(1);
}

const server = createServer({ port, apiKey });

// Handle shutdown gracefully
Deno.addSignalListener("SIGINT", () => {
  console.log("\nShutting down...");
  server.shutdown();
  Deno.exit(0);
});
