/**
 * Secmaster sync command - sync Kalshi events and markets to PostgreSQL
 */
import { getDb, closeDb, withTiming } from "../../lib/db/client.ts";
import { bulkUpsertEvents, softDeleteMissingEvents } from "../../lib/db/events.ts";
import { bulkUpsertMarkets, softDeleteMissingMarkets } from "../../lib/db/markets.ts";
import { createKalshiClient } from "../../lib/api/kalshi.ts";
import type { Event } from "../../lib/types/event.ts";
import type { Market } from "../../lib/types/market.ts";

/**
 * Secmaster sync options
 */
export interface SyncOptions {
  /** Sync only events, skip markets */
  eventsOnly?: boolean;
  /** Sync only markets, skip events */
  marketsOnly?: boolean;
  /** Skip soft-deleting missing records */
  noDelete?: boolean;
  /** Dry run - don't write to database */
  dryRun?: boolean;
}

/**
 * Sync result statistics
 */
export interface SyncResult {
  events: {
    fetched: number;
    upserted: number;
    deleted: number;
    durationMs: number;
  };
  markets: {
    fetched: number;
    upserted: number;
    skipped: number;
    deleted: number;
    durationMs: number;
  };
  totalDurationMs: number;
}

/**
 * Run the secmaster sync
 */
export async function runSecmasterSync(options: SyncOptions = {}): Promise<SyncResult> {
  const startTime = Date.now();
  const client = createKalshiClient();
  const sql = getDb();

  const result: SyncResult = {
    events: { fetched: 0, upserted: 0, deleted: 0, durationMs: 0 },
    markets: { fetched: 0, upserted: 0, skipped: 0, deleted: 0, durationMs: 0 },
    totalDurationMs: 0,
  };

  try {
    // Sync events
    if (!options.marketsOnly) {
      console.log("\n[Events] Starting sync...");
      const eventStart = Date.now();

      const allEvents: Event[] = [];
      for await (const batch of client.fetchAllEvents()) {
        allEvents.push(...batch);
      }
      result.events.fetched = allEvents.length;
      console.log(`[Events] Fetched ${allEvents.length} events from API`);

      if (!options.dryRun) {
        const { result: upsertResult } = await withTiming("events upsert", () =>
          bulkUpsertEvents(sql, allEvents)
        );
        result.events.upserted = upsertResult.total;
        console.log(`[Events] Upserted ${upsertResult.total} in ${upsertResult.batches} batches`);

        if (!options.noDelete) {
          const eventTickers = allEvents.map((e) => e.event_ticker);
          const deleted = await softDeleteMissingEvents(sql, eventTickers);
          result.events.deleted = deleted;
          if (deleted > 0) {
            console.log(`[Events] Soft-deleted ${deleted} missing events`);
          }
        }
      } else {
        console.log("[Events] Dry run - skipping database writes");
      }

      result.events.durationMs = Date.now() - eventStart;
    }

    // Sync markets
    if (!options.eventsOnly) {
      console.log("\n[Markets] Starting sync...");
      const marketStart = Date.now();

      const allMarkets: Market[] = [];
      for await (const batch of client.fetchAllMarkets()) {
        allMarkets.push(...batch);
      }
      result.markets.fetched = allMarkets.length;
      console.log(`[Markets] Fetched ${allMarkets.length} markets from API`);

      if (!options.dryRun) {
        const { result: upsertResult } = await withTiming("markets upsert", () =>
          bulkUpsertMarkets(sql, allMarkets)
        );
        result.markets.upserted = upsertResult.total;
        result.markets.skipped = upsertResult.skipped;
        console.log(
          `[Markets] Upserted ${upsertResult.total} in ${upsertResult.batches} batches` +
            (upsertResult.skipped > 0 ? ` (${upsertResult.skipped} skipped)` : "")
        );

        if (!options.noDelete) {
          const marketTickers = allMarkets.map((m) => m.ticker);
          const deleted = await softDeleteMissingMarkets(sql, marketTickers);
          result.markets.deleted = deleted;
          if (deleted > 0) {
            console.log(`[Markets] Soft-deleted ${deleted} missing markets`);
          }
        }
      } else {
        console.log("[Markets] Dry run - skipping database writes");
      }

      result.markets.durationMs = Date.now() - marketStart;
    }

    result.totalDurationMs = Date.now() - startTime;
    return result;
  } finally {
    await closeDb();
  }
}

/**
 * Print sync summary
 */
export function printSyncSummary(result: SyncResult): void {
  console.log("\n=== Secmaster Sync Summary ===");

  if (result.events.fetched > 0) {
    console.log(`Events:`);
    console.log(`  Fetched:  ${result.events.fetched}`);
    console.log(`  Upserted: ${result.events.upserted}`);
    console.log(`  Deleted:  ${result.events.deleted}`);
    console.log(`  Duration: ${(result.events.durationMs / 1000).toFixed(2)}s`);
  }

  if (result.markets.fetched > 0) {
    console.log(`Markets:`);
    console.log(`  Fetched:  ${result.markets.fetched}`);
    console.log(`  Upserted: ${result.markets.upserted}`);
    console.log(`  Skipped:  ${result.markets.skipped}`);
    console.log(`  Deleted:  ${result.markets.deleted}`);
    console.log(`  Duration: ${(result.markets.durationMs / 1000).toFixed(2)}s`);
  }

  console.log(`\nTotal time: ${(result.totalDurationMs / 1000).toFixed(2)}s`);
}

/**
 * Handle secmaster subcommands
 */
export async function handleSecmaster(
  subcommand: string,
  flags: Record<string, unknown>
): Promise<void> {
  switch (subcommand) {
    case "sync": {
      const options: SyncOptions = {
        eventsOnly: Boolean(flags["events-only"]),
        marketsOnly: Boolean(flags["markets-only"]),
        noDelete: Boolean(flags["no-delete"]),
        dryRun: Boolean(flags["dry-run"]),
      };

      if (options.eventsOnly && options.marketsOnly) {
        console.error("Cannot specify both --events-only and --markets-only");
        Deno.exit(1);
      }

      try {
        const result = await runSecmasterSync(options);
        printSyncSummary(result);
      } catch (e) {
        console.error(`Sync failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    default:
      console.log("Usage: ssmd secmaster <command>");
      console.log();
      console.log("Commands:");
      console.log("  sync         Sync events and markets from Kalshi API");
      console.log();
      console.log("Options for sync:");
      console.log("  --events-only    Only sync events");
      console.log("  --markets-only   Only sync markets");
      console.log("  --no-delete      Skip soft-deleting missing records");
      console.log("  --dry-run        Fetch but don't write to database");
      Deno.exit(1);
  }
}
