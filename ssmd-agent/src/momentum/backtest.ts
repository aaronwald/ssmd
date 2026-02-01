import { ensureDir } from "https://deno.land/std@0.224.0/fs/ensure_dir.ts";
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { TextLineStream } from "https://deno.land/std@0.224.0/streams/text_line_stream.ts";
import type { MomentumConfig } from "./config.ts";
import { createMomentumState, processRecord } from "./runner.ts";
import { parseMomentumRecord } from "./parse.ts";

interface BacktestOptions {
  config: MomentumConfig;
  dates: string[];
  bucket: string;
  prefix: string;
  tradesOut?: string;
}

function cacheDir(bucket: string, prefix: string, date: string): string {
  const home = Deno.env.get("HOME") ?? "/tmp";
  return join(home, ".cache", "ssmd-backtest", bucket, prefix, date);
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
async function ensureCached(gsUrl: string, bucket: string, prefix: string, date: string): Promise<string | null> {
  const fileName = gsUrl.split("/").pop() ?? "";
  const dir = cacheDir(bucket, prefix, date);
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
 * Files are cached locally at ~/.cache/ssmd-backtest/ for fast reruns.
 */
export async function runMomentumBacktest(options: BacktestOptions): Promise<void> {
  const { config, dates, bucket, prefix } = options;
  const state = createMomentumState(config);
  state.reporter.quiet = true;

  console.log(`[backtest] Momentum Backtest`);
  console.log(`[backtest] Signals: ${state.signals.map((s) => s.name).join(", ")}`);
  console.log(`[backtest] Portfolio: $${config.portfolio.startingBalance} balance, $${config.portfolio.tradeSize}/trade`);
  console.log(`[backtest] Activation: $${state.activationThreshold} in ${config.activation.windowMinutes}min`);
  console.log(`[backtest] Dates: ${dates.join(", ")}`);
  console.log(`[backtest] Source: gs://${bucket}/${prefix}/`);
  console.log(`[backtest] Cache: ${cacheDir(bucket, prefix, "")}`);
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
    const dir = cacheDir(bucket, prefix, date);
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
      const localPath = await ensureCached(gsUrl, bucket, prefix, date);
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
  }

  console.log(`\n[backtest] Complete. ${state.recordCount.toLocaleString()} records across ${totalFiles} files.`);
  if (parseErrors > 0) {
    console.log(`[backtest] Parse errors: ${parseErrors}`);
  }
  state.reporter.printSummary(state.pm);

  // Write per-trade JSONL if requested
  if (options.tradesOut && state.pm.closedPositions.length > 0) {
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
    await Deno.writeTextFile(options.tradesOut, lines.join("\n") + "\n");
    console.log(`[backtest] Wrote ${lines.length} trades to ${options.tradesOut}`);
  }
}
