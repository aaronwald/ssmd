import { ensureDir } from "https://deno.land/std@0.224.0/fs/ensure_dir.ts";
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { TextLineStream } from "https://deno.land/std@0.224.0/streams/text_line_stream.ts";
import type { MomentumConfig } from "./config.ts";
import { createMomentumState, processRecord } from "./runner.ts";
import { parseMomentumRecord } from "./parse.ts";

interface BacktestOptions {
  config: MomentumConfig;
  configPath: string;
  dates: string[];
  bucket: string;
  prefix: string;
  tradesOut?: string;
  cacheDir?: string;
  resultsDir?: string;
  runId?: string;
}

/**
 * Activate gcloud service account if GOOGLE_APPLICATION_CREDENTIALS is set.
 * Required for gcloud CLI commands (storage ls/cp) in containerized environments.
 */
async function activateGcloudAuth(): Promise<void> {
  const keyFile = Deno.env.get("GOOGLE_APPLICATION_CREDENTIALS");
  if (!keyFile) return;

  try {
    await Deno.stat(keyFile);
  } catch {
    return; // key file doesn't exist, skip
  }

  const cmd = new Deno.Command("gcloud", {
    args: ["auth", "activate-service-account", "--key-file", keyFile],
    stdout: "piped",
    stderr: "piped",
  });

  const output = await cmd.output();
  if (!output.success) {
    const err = new TextDecoder().decode(output.stderr);
    console.error(`[backtest] Warning: gcloud auth failed: ${err}`);
  }
}

function defaultCacheDir(bucket: string, prefix: string): string {
  const home = Deno.env.get("HOME") ?? "/tmp";
  return join(home, ".cache", "ssmd-backtest", bucket, prefix);
}

function cacheDirForDate(baseCache: string, date: string): string {
  return join(baseCache, date);
}

/**
 * List .jsonl.gz files in a GCS path for a given date.
 */
async function listGcsFiles(bucket: string, prefix: string, date: string): Promise<string[]> {
  const gsPath = `gs://${bucket}/${prefix}/${date}/`;
  const cmd = new Deno.Command("gcloud", {
    args: ["storage", "ls", gsPath],
    stdout: "piped",
    stderr: "piped",
  });

  const output = await cmd.output();
  if (!output.success) {
    const err = new TextDecoder().decode(output.stderr);
    if (err.includes("CommandException") || err.includes("NOT_FOUND") || err.includes("matched no objects")) {
      return [];
    }
    throw new Error(`gcloud storage ls failed for ${gsPath}: ${err}`);
  }

  const text = new TextDecoder().decode(output.stdout);
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.endsWith(".jsonl.gz"));
}

/**
 * Download a .jsonl.gz from GCS to local cache. Returns local path.
 * Skips download if already cached.
 */
async function ensureCached(gsUrl: string, baseCache: string, date: string): Promise<string | null> {
  const fileName = gsUrl.split("/").pop() ?? "";
  const dir = cacheDirForDate(baseCache, date);
  const localPath = join(dir, fileName);

  try {
    await Deno.stat(localPath);
    return localPath; // already cached
  } catch {
    // not cached, download
  }

  await ensureDir(dir);

  const cmd = new Deno.Command("gcloud", {
    args: ["storage", "cp", gsUrl, localPath],
    stdout: "piped",
    stderr: "piped",
  });

  const output = await cmd.output();
  if (!output.success) {
    const err = new TextDecoder().decode(output.stderr);
    console.error(`[backtest] download failed for ${gsUrl}: ${err}`);
    return null;
  }

  return localPath;
}

/**
 * Stream lines from a local .jsonl.gz file via gunzip.
 */
async function* readLocalJsonlGz(localPath: string): AsyncGenerator<string> {
  const gunzip = new Deno.Command("gunzip", {
    args: ["-c", localPath],
    stdout: "piped",
    stderr: "piped",
  });

  const child = gunzip.spawn();

  const lineStream = child.stdout
    .pipeThrough(new TextDecoderStream())
    .pipeThrough(new TextLineStream());

  for await (const line of lineStream) {
    if (line.length > 0) {
      yield line;
    }
  }

  const status = await child.status;
  if (!status.success) {
    console.error(`[backtest] gunzip error for ${localPath}`);
  }
}

/**
 * Run a momentum backtest over historical GCS archive data.
 * Files are cached locally for fast reruns. Results are written to a
 * run-specific directory with summary.json and trades.jsonl.
 */
export async function runMomentumBacktest(options: BacktestOptions): Promise<void> {
  const { config, configPath, dates, bucket, prefix } = options;
  const runId = options.runId ?? crypto.randomUUID();
  const baseCache = options.cacheDir ?? defaultCacheDir(bucket, prefix);
  const resultsDir = options.resultsDir ?? "./results";

  await activateGcloudAuth();

  const state = createMomentumState(config);
  state.reporter.quiet = true;

  console.log(`[backtest] Momentum Backtest`);
  console.log(`[backtest] Run ID: ${runId}`);
  console.log(`[backtest] Signals: ${state.signals.map((s) => s.name).join(", ")}`);
  console.log(`[backtest] Portfolio: $${config.portfolio.startingBalance} balance, $${config.portfolio.tradeSize}/trade`);
  console.log(`[backtest] Activation: $${state.activationThreshold} in ${config.activation.windowMinutes}min`);
  console.log(`[backtest] Dates: ${dates.join(", ")}`);
  console.log(`[backtest] Source: gs://${bucket}/${prefix}/`);
  console.log(`[backtest] Cache: ${baseCache}`);
  console.log(``);

  let totalFiles = 0;
  let parseErrors = 0;
  let cacheHits = 0;

  for (const date of dates) {
    console.log(`[backtest] Processing ${date}...`);

    const files = await listGcsFiles(bucket, prefix, date);
    if (files.length === 0) {
      console.log(`[backtest]   No files found for ${date}`);
      continue;
    }

    files.sort();

    // Check how many are already cached
    const dir = cacheDirForDate(baseCache, date);
    let cached = 0;
    try {
      for await (const entry of Deno.readDir(dir)) {
        if (entry.name.endsWith(".jsonl.gz")) cached++;
      }
    } catch {
      // dir doesn't exist yet
    }
    const downloading = files.length - cached;
    if (downloading > 0) {
      console.log(`[backtest]   ${files.length} files (${cached} cached, downloading ${downloading})`);
    } else {
      console.log(`[backtest]   ${files.length} files (all cached)`);
    }

    for (const gsUrl of files) {
      const localPath = await ensureCached(gsUrl, baseCache, date);
      if (!localPath) continue;

      if (cached > 0) cacheHits++;

      try {
        for await (const line of readLocalJsonlGz(localPath)) {
          try {
            const raw = JSON.parse(line);
            const record = parseMomentumRecord(raw);
            if (record) {
              processRecord(record, state);
            }
          } catch {
            parseErrors++;
          }
        }
      } catch (e) {
        const fileName = localPath.split("/").pop() ?? localPath;
        console.error(`[backtest]   Error reading ${fileName}: ${e}`);
      }
    }

    totalFiles += files.length;

    if (state.pm.isHalted) {
      console.log(`[backtest] Portfolio halted — stopping early.`);
      break;
    }
  }

  console.log(`\n[backtest] Complete. ${state.recordCount.toLocaleString()} records across ${totalFiles} files.`);
  if (parseErrors > 0) {
    console.log(`[backtest] Parse errors: ${parseErrors}`);
  }
  state.reporter.printSummary(state.pm);

  // Write results to {resultsDir}/{runId}/
  const runDir = join(resultsDir, runId);
  await ensureDir(runDir);

  // Build per-model stats
  const byModel = new Map<string, { trades: number; wins: number; losses: number; pnl: number }>();
  for (const c of state.pm.closedPositions) {
    const model = c.position.model;
    const entry = byModel.get(model) ?? { trades: 0, wins: 0, losses: 0, pnl: 0 };
    entry.trades++;
    if (c.pnl > 0) entry.wins++;
    else entry.losses++;
    entry.pnl += c.pnl;
    byModel.set(model, entry);
  }

  const pmSummary = state.pm.getSummary();

  const summary = {
    runId,
    timestamp: new Date().toISOString(),
    config: configPath,
    dates: { from: dates[0], to: dates[dates.length - 1], count: dates.length },
    source: { bucket, prefix },
    records: state.recordCount,
    files: totalFiles,
    parseErrors,
    results: Array.from(byModel.entries()).map(([model, stats]) => ({
      model,
      trades: stats.trades,
      wins: stats.wins,
      losses: stats.losses,
      winRate: stats.trades > 0 ? stats.wins / stats.trades : 0,
      netPnl: stats.pnl,
    })),
    portfolio: {
      startingBalance: state.pm.startingBalance,
      balance: pmSummary.balance,
      totalPnl: pmSummary.totalPnl,
      drawdownPercent: pmSummary.drawdownPercent,
      halted: state.pm.isHalted,
    },
  };

  await Deno.writeTextFile(join(runDir, "summary.json"), JSON.stringify(summary, null, 2));
  console.log(`[backtest] Wrote summary to ${join(runDir, "summary.json")}`);

  // Always write trades JSONL
  if (state.pm.closedPositions.length > 0) {
    const lines = state.pm.closedPositions.map((c) =>
      JSON.stringify({
        model: c.position.model,
        ticker: c.position.ticker,
        side: c.position.side,
        entryPrice: c.position.entryPrice,
        exitPrice: c.exitPrice,
        contracts: c.position.contracts,
        entryTime: c.position.entryTime,
        exitTime: c.exitTime,
        reason: c.reason,
        pnl: c.pnl,
        fees: c.fees,
        entryCost: c.position.entryCost,
      })
    );
    const tradesPath = join(runDir, "trades.jsonl");
    await Deno.writeTextFile(tradesPath, lines.join("\n") + "\n");
    console.log(`[backtest] Wrote ${lines.length} trades to ${tradesPath}`);

    // Also write to explicit --trades-out if specified
    if (options.tradesOut) {
      await Deno.writeTextFile(options.tradesOut, lines.join("\n") + "\n");
      console.log(`[backtest] Wrote ${lines.length} trades to ${options.tradesOut}`);
    }
  }
}
