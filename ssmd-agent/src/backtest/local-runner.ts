// ssmd-agent/src/backtest/local-runner.ts
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { gunzip } from "https://deno.land/x/compress@v0.4.6/gzip/mod.ts";
import type { MarketRecord, StateBuilder } from "../state/types.ts";
import { VolumeProfileBuilder } from "../state/volume_profile.ts";
import { loadSignal, type LoadedSignal } from "./loader.ts";

export interface BacktestFire {
  ts: number;
  ticker: string;
  payload: unknown;
}

export interface LocalBacktestResult {
  signalId: string;
  dates: string[];
  feed: string;
  recordsProcessed: number;
  tickersProcessed: number;
  fires: BacktestFire[];
  errors: string[];
  durationMs: number;
}

interface RawRecord {
  type: string;
  sid?: number;
  msg?: Record<string, unknown>;
}

/**
 * Parse a raw JSONL record into a MarketRecord
 */
function parseRecord(raw: RawRecord): MarketRecord | null {
  if (!raw.msg) return null;

  const msg = raw.msg;
  return {
    type: raw.type,
    ticker: (msg.market_ticker as string) ?? "",
    ts: (msg.ts as number) ?? 0,
    volume: msg.volume as number | undefined,
    dollar_volume: msg.dollar_volume as number | undefined,
    price: msg.price as number | undefined,
    yes_bid: msg.yes_bid as number | undefined,
    yes_ask: msg.yes_ask as number | undefined,
  };
}

/**
 * Create a state builder by name
 */
function createBuilder(name: string, config?: Record<string, unknown>): StateBuilder<unknown> | null {
  switch (name) {
    case "volumeProfile": {
      const windowMs = (config?.windowMs as number) ?? 1800000; // 30 min default
      return new VolumeProfileBuilder(windowMs);
    }
    default:
      return null;
  }
}

/**
 * Read and decompress a .jsonl.gz file
 */
async function readGzipJsonl(path: string): Promise<string[]> {
  const compressed = await Deno.readFile(path);
  const decompressed = gunzip(compressed);
  const text = new TextDecoder().decode(decompressed);
  return text.trim().split("\n").filter(Boolean);
}

/**
 * Run a local backtest against JSONL data files
 */
export async function runLocalBacktest(
  signalPath: string,
  dataDir: string,
  dates: string[],
  feed: string = "kalshi",
  stateConfig?: Record<string, Record<string, unknown>>
): Promise<LocalBacktestResult> {
  const start = Date.now();
  const errors: string[] = [];
  const fires: BacktestFire[] = [];
  let recordsProcessed = 0;
  const tickersSeen = new Set<string>();

  // Load signal
  const signal = await loadSignal(signalPath);

  // Compile signal evaluate/payload functions
  const signalModule = await compileSignal(signal);

  // Create builders per ticker (will be created on demand)
  const tickerBuilders = new Map<string, Map<string, StateBuilder<unknown>>>();

  // Process each date
  for (const date of dates) {
    const dateDir = join(dataDir, feed, date);

    // List files for this date
    let files: string[];
    try {
      const entries = [];
      for await (const entry of Deno.readDir(dateDir)) {
        if (entry.isFile && entry.name.endsWith(".jsonl.gz")) {
          entries.push(entry.name);
        }
      }
      files = entries.sort(); // Process in chronological order
    } catch (e) {
      errors.push(`Failed to read ${dateDir}: ${e}`);
      continue;
    }

    console.log(`Processing ${date}: ${files.length} files`);

    for (const file of files) {
      const filePath = join(dateDir, file);

      try {
        const lines = await readGzipJsonl(filePath);

        for (const line of lines) {
          try {
            const raw = JSON.parse(line) as RawRecord;
            const record = parseRecord(raw);
            if (!record || !record.ticker) continue;

            recordsProcessed++;
            tickersSeen.add(record.ticker);

            // Get or create builders for this ticker
            let builders = tickerBuilders.get(record.ticker);
            if (!builders) {
              builders = new Map();
              for (const req of signal.requires) {
                const config = stateConfig?.[req];
                const builder = createBuilder(req, config);
                if (builder) {
                  builders.set(req, builder);
                }
              }
              tickerBuilders.set(record.ticker, builders);
            }

            // Update all builders
            for (const builder of builders.values()) {
              builder.update(record);
            }

            // Build state map for signal evaluation
            const stateMap: Record<string, unknown> = {};
            for (const [name, builder] of builders) {
              stateMap[name] = builder.getState();
            }

            // Evaluate signal
            try {
              if (signalModule.evaluate(stateMap)) {
                const payload = signalModule.payload(stateMap);
                fires.push({
                  ts: record.ts,
                  ticker: record.ticker,
                  payload,
                });
              }
            } catch (e) {
              if (errors.length < 10) {
                errors.push(`Signal error for ${record.ticker}: ${e}`);
              }
            }
          } catch (e) {
            if (errors.length < 10) {
              errors.push(`Parse error in ${file}: ${e}`);
            }
          }
        }
      } catch (e) {
        errors.push(`Failed to process ${file}: ${e}`);
      }
    }
  }

  return {
    signalId: signal.id,
    dates,
    feed,
    recordsProcessed,
    tickersProcessed: tickersSeen.size,
    fires,
    errors,
    durationMs: Date.now() - start,
  };
}

interface SignalModule {
  evaluate: (state: Record<string, unknown>) => boolean;
  payload: (state: Record<string, unknown>) => unknown;
}

/**
 * Compile signal code into executable functions
 */
async function compileSignal(signal: LoadedSignal): Promise<SignalModule> {
  // Import the signal module directly
  const modulePath = new URL(signal.path, `file://${Deno.cwd()}/`).href;

  try {
    const module = await import(modulePath);
    const sig = module.signal;

    if (!sig || typeof sig.evaluate !== "function") {
      throw new Error("Signal must export { signal } with evaluate function");
    }

    return {
      evaluate: sig.evaluate,
      payload: sig.payload ?? (() => ({})),
    };
  } catch (e) {
    throw new Error(`Failed to load signal: ${e}`);
  }
}

/**
 * Format a backtest result for display
 */
export function formatResult(result: LocalBacktestResult): string {
  const lines: string[] = [];

  lines.push(`=== Backtest Results: ${result.signalId} ===`);
  lines.push("");
  lines.push(`Dates:    ${result.dates.join(", ")}`);
  lines.push(`Feed:     ${result.feed}`);
  lines.push(`Duration: ${(result.durationMs / 1000).toFixed(1)}s`);
  lines.push("");
  lines.push(`Records:  ${result.recordsProcessed.toLocaleString()}`);
  lines.push(`Tickers:  ${result.tickersProcessed.toLocaleString()}`);
  lines.push(`Fires:    ${result.fires.length}`);

  if (result.errors.length > 0) {
    lines.push("");
    lines.push(`Errors:   ${result.errors.length}`);
    for (const err of result.errors.slice(0, 5)) {
      lines.push(`  - ${err}`);
    }
  }

  if (result.fires.length > 0) {
    lines.push("");
    lines.push("Sample Fires:");
    for (const fire of result.fires.slice(0, 10)) {
      // ts is already in seconds, convert to ms for Date
      const time = new Date(fire.ts * 1000).toISOString();
      lines.push(`  ${time} ${fire.ticker}`);
      lines.push(`    ${JSON.stringify(fire.payload)}`);
    }

    if (result.fires.length > 10) {
      lines.push(`  ... and ${result.fires.length - 10} more`);
    }
  }

  return lines.join("\n");
}
