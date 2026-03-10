import { startWorker } from "./worker.ts";

function requireEnv(name: string): string {
  const value = Deno.env.get(name);
  if (!value) {
    console.error(`[main] missing required env var: ${name}`);
    Deno.exit(1);
  }
  return value;
}

const config = {
  databaseUrl: requireEnv("DATABASE_URL"),
  databaseUrlReadonly: requireEnv("DATABASE_URL_READONLY"),
  dataTsUrl: Deno.env.get("DATA_TS_INTERNAL_URL") ?? "http://ssmd-data-ts-internal:8081",
  adminApiKey: requireEnv("PIPELINE_ADMIN_API_KEY"),
  pollIntervalMs: parseInt(Deno.env.get("POLL_INTERVAL_MS") ?? "5000", 10),
};

console.log("[main] ssmd-pipeline-worker starting...");
console.log(`[main] poll_interval=${config.pollIntervalMs}ms data_ts=${config.dataTsUrl}`);

await startWorker(config);
