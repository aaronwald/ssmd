/**
 * Build-time only: materialise the FULL runtime module graph into DENO_DIR.
 *
 * `deno cache <entrypoint>` bakes the statically-resolvable graph, but the npm
 * *runtime* graph is materialised by the module loader on first execution:
 *
 *   - drizzle-orm's optional peerDependencies (pg, mysql2, @neondatabase/serverless,
 *     better-sqlite3, sql.js, prisma, planetscale, @aws-sdk/client-rds-data, ...)
 *   - platform-specific native bindings (@duckdb/node-bindings-*, @google-cloud/storage)
 *   - nats' lazily-imported nkeys.js
 *
 * Measured on a clean DENO_DIR: `deno cache` fetched 250 packages, and the first
 * `deno run` of the CLI then fetched 138 MORE from registry.npmjs.org. Every
 * CronJob pod paid that cost on every start — `ssmd-hols-binance-1m-intraday`
 * runs every 5 minutes, so ~288 times a day. That resolution is time-variable:
 * it has hung until the pod deadline (DeadlineExceeded) and OOMKilled at both
 * 1Gi and 2Gi, and its memory spike is what made the intraday job exceed its
 * request and get evicted 146x in 24h on 2026-07-26.
 *
 * Importing a module is what forces its npm packages to be resolved and
 * extracted, so this file must import the command modules that `commands/mod.ts`
 * loads via `await import(...)` — a `--help` invocation returns before the
 * dispatch switch (`commands/mod.ts`: `if (flags.help || !command)`) and would
 * leave those graphs cold.
 *
 * Both consumer modules guard their entrypoints with `if (import.meta.main)`,
 * which is false when they are imported, so this has no side effects beyond
 * populating the cache.
 *
 * Not referenced by any runtime code path. Invoked once from Dockerfile.data.
 */

// Statically-dispatched commands + the server (shared lib/db, nats, drizzle).
import "./commands/mod.ts";
import "../server/mod.ts";

// Dynamically-imported commands — cold unless imported explicitly. hols pulls
// the heaviest set (DuckDB native bindings, GCS, nodemailer).
import "./commands/hols.ts";
import "./commands/retention.ts";
import "./commands/funding-rate-consumer.ts";
import "./commands/lifecycle-consumer.ts";

console.log("[warm-cache] full runtime module graph materialised into DENO_DIR");
