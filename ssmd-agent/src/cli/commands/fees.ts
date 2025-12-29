/**
 * Fees sync command - sync Kalshi fee schedules to PostgreSQL
 */
import { getDb, closeDb, upsertFeeChanges, getFeeStats, listCurrentFees } from "../../lib/db/mod.ts";
import { createKalshiClient } from "../../lib/api/kalshi.ts";

/**
 * Fee sync options
 */
export interface FeeSyncOptions {
  /** Dry run - don't write to database */
  dryRun?: boolean;
}

/**
 * Fee sync result
 */
export interface FeeSyncResult {
  fetched: number;
  inserted: number;
  skipped: number;
  durationMs: number;
}

/**
 * Run the fees sync
 */
export async function runFeesSync(options: FeeSyncOptions = {}): Promise<FeeSyncResult> {
  const startTime = Date.now();
  const client = createKalshiClient();
  const sql = getDb();

  const result: FeeSyncResult = {
    fetched: 0,
    inserted: 0,
    skipped: 0,
    durationMs: 0,
  };

  try {
    console.log("\n[Fees] Fetching fee changes from Kalshi API...");

    // Fetch all fee changes (including historical)
    const feeChanges = await client.fetchFeeChanges(true);
    result.fetched = feeChanges.length;

    console.log(`[Fees] Fetched ${result.fetched} fee changes`);

    if (!options.dryRun) {
      // Upsert to database
      const dbResult = await upsertFeeChanges(sql, feeChanges);
      result.inserted = dbResult.inserted;
      result.skipped = dbResult.skipped;
    } else {
      console.log("[Fees] Dry run - skipping database writes");
    }

    result.durationMs = Date.now() - startTime;
    return result;
  } finally {
    await closeDb();
  }
}

/**
 * Print sync summary
 */
export function printSyncSummary(result: FeeSyncResult): void {
  console.log("\n=== Fees Sync Summary ===");
  console.log(`Fetched:  ${result.fetched}`);
  console.log(`Inserted: ${result.inserted}`);
  console.log(`Skipped:  ${result.skipped} (duplicates)`);
  console.log(`Duration: ${(result.durationMs / 1000).toFixed(2)}s`);
}

/**
 * Show fee statistics
 */
export async function showFeeStats(): Promise<void> {
  const sql = getDb();

  try {
    const stats = await getFeeStats(sql);
    console.log("\n=== Fee Schedule Statistics ===");
    console.log(`Total records:    ${stats.total}`);
    console.log(`Active schedules: ${stats.active}`);
    console.log(`Historical:       ${stats.historical}`);
  } finally {
    await closeDb();
  }
}

/**
 * List current fee schedules
 */
export async function showFeeList(limit = 50): Promise<void> {
  const sql = getDb();

  try {
    const fees = await listCurrentFees(sql, limit);

    if (fees.length === 0) {
      console.log("\nNo fee schedules found. Run 'ssmd fees sync' first.");
      return;
    }

    console.log(`\n=== Current Fee Schedules (${fees.length}) ===`);
    console.log("");
    console.log("Series Ticker       Fee Type                    Multiplier  Effective From");
    console.log("-".repeat(80));

    for (const fee of fees) {
      const ticker = fee.series_ticker.padEnd(18);
      const type = fee.fee_type.padEnd(26);
      const mult = fee.fee_multiplier.toFixed(4).padStart(10);
      const from = fee.effective_from.toISOString().slice(0, 10);
      console.log(`${ticker}  ${type}  ${mult}  ${from}`);
    }
  } finally {
    await closeDb();
  }
}

/**
 * Handle fees subcommands
 */
export async function handleFees(
  subcommand: string,
  flags: Record<string, unknown>
): Promise<void> {
  switch (subcommand) {
    case "sync": {
      const options: FeeSyncOptions = {
        dryRun: Boolean(flags["dry-run"]),
      };

      try {
        const result = await runFeesSync(options);
        printSyncSummary(result);
      } catch (e) {
        console.error(`Fees sync failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    case "stats": {
      try {
        await showFeeStats();
      } catch (e) {
        console.error(`Failed to get stats: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    case "list": {
      const limit = flags.limit ? Number(flags.limit) : 50;
      try {
        await showFeeList(limit);
      } catch (e) {
        console.error(`Failed to list fees: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    default:
      console.log("Usage: ssmd fees <command>");
      console.log();
      console.log("Commands:");
      console.log("  sync         Sync fee schedules from Kalshi API");
      console.log("  stats        Show fee schedule statistics");
      console.log("  list         List current fee schedules");
      console.log();
      console.log("Options for sync:");
      console.log("  --dry-run    Fetch but don't write to database");
      console.log();
      console.log("Options for list:");
      console.log("  --limit N    Maximum records to show (default: 50)");
      Deno.exit(1);
  }
}
