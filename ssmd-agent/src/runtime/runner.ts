// ssmd-agent/src/runtime/runner.ts
import type { StateBuilder } from "../state/types.ts";
import { VolumeProfileBuilder } from "../state/volume_profile.ts";
import { loadSignal, type LoadedSignal } from "../backtest/loader.ts";
import type { RuntimeConfig, SignalFire } from "./interfaces.ts";

/**
 * Compiled signal module with evaluate and payload functions
 */
interface SignalModule {
  evaluate: (state: Record<string, unknown>) => boolean;
  payload: (state: Record<string, unknown>) => unknown;
}

/**
 * Create a state builder by name with optional configuration
 */
export function createBuilder(
  name: string,
  config?: Record<string, unknown>
): StateBuilder<unknown> | null {
  switch (name) {
    case "volumeProfile": {
      const windowMs = (config?.windowMs as number) ?? 1800000; // 30 min default
      return new VolumeProfileBuilder(windowMs);
    }
    // Add more builders here as needed:
    // case "orderbook":
    //   return new OrderBookBuilder();
    // case "priceHistory":
    //   return new PriceHistoryBuilder();
    default:
      return null;
  }
}

/**
 * Get or create state builders for a ticker
 */
function getOrCreateBuilders(
  tickerBuilders: Map<string, Map<string, StateBuilder<unknown>>>,
  ticker: string,
  requires: string[],
  stateConfig?: Record<string, Record<string, unknown>>
): Map<string, StateBuilder<unknown>> {
  let builders = tickerBuilders.get(ticker);
  if (!builders) {
    builders = new Map();
    for (const req of requires) {
      const config = stateConfig?.[req];
      const builder = createBuilder(req, config);
      if (builder) {
        builders.set(req, builder);
      }
    }
    tickerBuilders.set(ticker, builders);
  }
  return builders;
}

/**
 * Build state map from builders for signal evaluation
 */
function buildStateMap(
  builders: Map<string, StateBuilder<unknown>>
): Record<string, unknown> {
  const state: Record<string, unknown> = {};
  for (const [name, builder] of builders) {
    state[name] = builder.getState();
  }
  return state;
}

/**
 * Compile signal code into executable functions
 */
async function compileSignal(signal: LoadedSignal): Promise<SignalModule> {
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
 * Runtime statistics
 */
export interface RuntimeStats {
  recordsProcessed: number;
  tickersTracked: number;
  firesPublished: number;
  errors: number;
  startTime: number;
}

/**
 * Run a signal against a record source, publishing fires to a sink.
 * This is the core runtime loop.
 */
export async function runSignal(config: RuntimeConfig): Promise<RuntimeStats> {
  const stats: RuntimeStats = {
    recordsProcessed: 0,
    tickersTracked: 0,
    firesPublished: 0,
    errors: 0,
    startTime: Date.now(),
  };

  // Load and compile signal
  const signal = await loadSignal(config.signalPath);
  const signalModule = await compileSignal(signal);

  console.log(`Starting signal runtime: ${signal.id}`);
  console.log(`  Requires: ${signal.requires.join(", ")}`);

  // State builders per ticker
  const tickerBuilders = new Map<string, Map<string, StateBuilder<unknown>>>();

  // Process records from source
  try {
    for await (const record of config.source.subscribe()) {
      if (!record.ticker) continue;

      stats.recordsProcessed++;

      // Get or create builders for this ticker
      const builders = getOrCreateBuilders(
        tickerBuilders,
        record.ticker,
        signal.requires,
        config.stateConfig
      );

      // Update stats on first see
      if (builders.size > 0 && !tickerBuilders.has(record.ticker)) {
        stats.tickersTracked++;
      }

      // Update all builders with this record
      for (const builder of builders.values()) {
        builder.update(record);
      }

      // Build state map and evaluate signal
      const state = buildStateMap(builders);

      try {
        if (signalModule.evaluate(state)) {
          const fire: SignalFire = {
            signalId: signal.id,
            ts: record.ts,
            ticker: record.ticker,
            payload: signalModule.payload(state),
          };

          await config.sink.publish(fire);
          stats.firesPublished++;
        }
      } catch (e) {
        stats.errors++;
        if (stats.errors <= 10) {
          console.error(`Signal error for ${record.ticker}: ${e}`);
        }
      }

      // Log stats periodically
      if (stats.recordsProcessed % 100000 === 0) {
        const elapsed = (Date.now() - stats.startTime) / 1000;
        console.log(
          `Processed ${stats.recordsProcessed.toLocaleString()} records, ` +
          `${tickerBuilders.size} tickers, ${stats.firesPublished} fires ` +
          `(${elapsed.toFixed(1)}s)`
        );
      }
    }
  } finally {
    stats.tickersTracked = tickerBuilders.size;
    await config.source.close();
    await config.sink.close();
  }

  return stats;
}
