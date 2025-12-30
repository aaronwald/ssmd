// ssmd-agent/src/cli/commands/signal-runner.ts
// Daemon mode for running multiple signals against NATS

import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { config } from "../../config.ts";
import { runSignals } from "../../runtime/runner.ts";
import { NatsRecordSource, NatsFireSink, LoggingFireSink } from "../../runtime/nats.ts";

/**
 * Run the signal runner daemon.
 * Reads SIGNALS env var and runs all specified signals.
 */
export async function runDaemon(): Promise<void> {
  console.log("=== SSMD Signal Runner ===");
  console.log();

  // Get signals from config
  const signalNames = config.signals;
  if (signalNames.length === 0) {
    console.error("No signals specified. Set SIGNALS env var (comma-separated)");
    console.error("Example: SIGNALS=volume-1m-30min,other-signal");
    Deno.exit(1);
  }

  console.log(`Signals: ${signalNames.join(", ")}`);
  console.log(`NATS: ${config.natsUrl}`);
  console.log(`Stream: ${config.natsStream}`);
  console.log();

  // Build signal paths
  const signalPaths = signalNames.map(name => join(config.signalsPath, name));

  // Verify all signals exist
  for (const path of signalPaths) {
    try {
      await Deno.stat(join(path, "signal.ts"));
    } catch {
      console.error(`Signal not found: ${path}/signal.ts`);
      Deno.exit(1);
    }
  }

  // Create NATS source and sink
  const source = new NatsRecordSource(
    config.natsUrl,
    config.natsStream,
    "prod.kalshi.>" // Subscribe to all Kalshi data
  );

  // Wrap NATS sink with logging
  const natsSink = new NatsFireSink(config.natsUrl);
  const sink = new LoggingFireSink(natsSink);

  // Run signals
  try {
    const stats = await runSignals({
      signalPaths,
      source,
      sink,
    });

    console.log();
    console.log("=== Signal Runner Stopped ===");
    console.log(`Records: ${stats.recordsProcessed.toLocaleString()}`);
    console.log(`Tickers: ${stats.tickersTracked.toLocaleString()}`);
    console.log(`Fires: ${stats.firesPublished}`);
    console.log(`Errors: ${stats.errors}`);
  } catch (e) {
    console.error(`Signal runner error: ${e}`);
    Deno.exit(1);
  }
}

// If run directly, start the daemon
if (import.meta.main) {
  await runDaemon();
}
