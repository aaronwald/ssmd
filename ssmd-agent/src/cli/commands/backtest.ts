import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import {
  loadSignal,
  getGitSha,
  isSignalDirty,
  expandDateRange as expandRange,
  getEffectiveDates,
} from "../../backtest/loader.ts";
import {
  runLocalBacktest,
  formatResult,
} from "../../backtest/local-runner.ts";
import type { BacktestResult } from "../../lib/types/backtest.ts";
import { TablePrinter } from "../utils/table.ts";

/**
 * Parsed backtest command arguments
 */
export interface BacktestArgs {
  signal: string;
  dates: string[];
  sha?: string;
  allowDirty: boolean;
  noWait: boolean;
  feed?: string;
}

/**
 * Parse flags into structured backtest arguments
 */
export function parseBacktestArgs(flags: Record<string, unknown>): BacktestArgs {
  const args = flags._ as string[];
  const signal = args[2] as string;

  let dates: string[] = [];
  if (flags.dates) {
    dates = (flags.dates as string).split(",").map((d) => d.trim());
  } else if (flags.from && flags.to) {
    dates = expandDateRange(flags.from as string, flags.to as string);
  }

  return {
    signal,
    dates,
    sha: flags.sha as string | undefined,
    allowDirty: Boolean(flags["allow-dirty"]),
    noWait: Boolean(flags["no-wait"]),
    feed: flags.feed as string | undefined,
  };
}

/**
 * Expand a date range into an array of YYYY-MM-DD strings
 */
export function expandDateRange(from: string, to: string): string[] {
  return expandRange(from, to);
}

/**
 * Run backtest command handler
 */
export async function runBacktestCommand(args: BacktestArgs): Promise<void> {
  const signalsDir = "signals";
  const signalPath = join(signalsDir, args.signal);

  // Check if signal exists
  try {
    await Deno.stat(join(signalPath, "signal.ts"));
  } catch {
    console.error(`Signal not found: ${signalPath}/signal.ts`);
    console.error("\nAvailable signals:");
    try {
      for await (const entry of Deno.readDir(signalsDir)) {
        if (entry.isDirectory) {
          try {
            await Deno.stat(join(signalsDir, entry.name, "signal.ts"));
            console.error(`  ${entry.name}`);
          } catch {
            // Not a signal directory
          }
        }
      }
    } catch {
      console.error("  (signals directory not found)");
    }
    Deno.exit(1);
  }

  // Check dirty state
  if (!args.allowDirty) {
    try {
      const dirty = await isSignalDirty(signalPath);
      if (dirty) {
        console.error(`Error: ${signalPath} has uncommitted changes`);
        console.error("Use --allow-dirty to run anyway, or commit changes first");
        Deno.exit(1);
      }
    } catch {
      // Git not available or not a git repo, continue
    }
  }

  // Load signal
  const signal = await loadSignal(signalPath);

  // Get git SHA
  let sha = args.sha;
  if (!sha) {
    try {
      sha = await getGitSha();
    } catch {
      sha = "unknown";
    }
  }

  // Get effective dates
  const dates = getEffectiveDates(signal.manifest, args.dates);
  const feed = args.feed ?? signal.manifest?.feed ?? "kalshi";

  if (dates.length === 0) {
    console.error("No dates specified. Use --dates or --from/--to, or add backtest.yaml");
    Deno.exit(1);
  }

  console.log(`Running backtest: ${signal.id}`);
  console.log(`  SHA:     ${sha}${args.allowDirty ? " (dirty)" : ""}`);
  console.log(`  Feed:    ${feed}`);
  console.log(`  Dates:   ${dates.length === 1 ? dates[0] : `${dates[0]} to ${dates[dates.length - 1]} (${dates.length} days)`}`);
  console.log(`  Requires: ${signal.requires.join(", ")}`);
  console.log();

  // Get state config from manifest
  const stateConfig = signal.manifest?.state as Record<string, Record<string, unknown>> | undefined;

  // Run local backtest
  const dataDir = "data";
  const result = await runLocalBacktest(signalPath, dataDir, dates, feed, stateConfig);

  // Display results
  console.log();
  console.log(formatResult(result));
}

/**
 * Check backtest status command handler
 */
export async function statusBacktestCommand(runId: string): Promise<void> {
  console.log(`Checking status for: ${runId}`);
  console.log();

  // TODO: Query Temporal for workflow status
  console.log("[TODO: Query Temporal for run status]");
  console.log();
  console.log("Status: PENDING (mock)");
  console.log("Progress: 0/0 records");
}

/**
 * Get backtest results command handler
 */
export async function resultsBacktestCommand(runId: string): Promise<void> {
  console.log(`Results for: ${runId}`);
  console.log();

  // TODO: Fetch results from Temporal or storage
  console.log("[TODO: Fetch results from storage]");
  console.log();

  // Mock result display
  const mockResult: Partial<BacktestResult> = {
    run_id: runId,
    results: {
      fire_count: 0,
      fire_rate: 0,
      fires: [],
    },
  };

  console.log("Results:");
  console.log(`  Fires:      ${mockResult.results?.fire_count}`);
  console.log(`  Fire Rate:  ${mockResult.results?.fire_rate}`);
}

/**
 * List recent backtest runs command handler
 */
export async function listBacktestsCommand(limit = 20): Promise<void> {
  console.log("Recent backtest runs:");
  console.log();

  // TODO: Query Temporal for recent workflow runs
  console.log("[TODO: Query Temporal for recent runs]");
  console.log();

  const table = new TablePrinter();
  table.header("RUN ID", "SIGNAL", "STATUS", "DATES", "FIRES");
  table.row("(no runs found)", "-", "-", "-", "-");
  table.flush();
}

/**
 * Handle backtest subcommands
 */
export async function handleBacktest(
  subcommand: string,
  flags: Record<string, unknown>
): Promise<void> {
  switch (subcommand) {
    case "run": {
      const args = parseBacktestArgs(flags);
      if (!args.signal) {
        console.error("Usage: ssmd backtest run <signal> [options]");
        console.error();
        console.error("Options:");
        console.error("  --dates X,Y,Z      Specific dates to backtest");
        console.error("  --from DATE        Start date (use with --to)");
        console.error("  --to DATE          End date (use with --from)");
        console.error("  --feed NAME        Override feed from manifest");
        console.error("  --sha SHA          Use specific git SHA");
        console.error("  --allow-dirty      Allow uncommitted changes");
        console.error("  --no-wait          Don't wait for completion");
        Deno.exit(1);
      }
      await runBacktestCommand(args);
      break;
    }

    case "status": {
      const runId = (flags._ as string[])[2];
      if (!runId) {
        console.error("Usage: ssmd backtest status <run-id>");
        Deno.exit(1);
      }
      await statusBacktestCommand(runId);
      break;
    }

    case "results": {
      const runId = (flags._ as string[])[2];
      if (!runId) {
        console.error("Usage: ssmd backtest results <run-id>");
        Deno.exit(1);
      }
      await resultsBacktestCommand(runId);
      break;
    }

    case "list":
      await listBacktestsCommand();
      break;

    default:
      console.log("Usage: ssmd backtest <command>");
      console.log();
      console.log("Commands:");
      console.log("  run <signal>    Run a backtest");
      console.log("  status <id>     Check backtest status");
      console.log("  results <id>    Get backtest results");
      console.log("  list            List recent backtests");
      Deno.exit(1);
  }
}
