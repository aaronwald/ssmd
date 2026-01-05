// ssmd-agent/src/cli/commands/signal-runner.ts
// Daemon mode for running multiple signals against NATS

import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { parseArgs } from "https://deno.land/std@0.224.0/cli/parse_args.ts";
import { config, loadSignalRunnerConfig, type SignalRunnerConfig } from "../../config.ts";
import { runSignals } from "../../runtime/runner.ts";
import { NatsRecordSource, NatsFireSink, LoggingFireSink } from "../../runtime/nats.ts";

/**
 * Run the signal runner daemon.
 * Reads configuration from --config file or falls back to env vars.
 */
export async function runDaemon(args: string[] = Deno.args): Promise<void> {
  const flags = parseArgs(args, {
    string: ["config"],
    alias: { c: "config" },
  });

  console.log("=== SSMD Signal Runner ===");
  console.log();

  // Load configuration from file or env vars
  const configPath = flags.config;
  let runnerConfig: SignalRunnerConfig;

  if (configPath) {
    console.log(`Config: ${configPath}`);
    runnerConfig = await loadSignalRunnerConfig(configPath);
  } else {
    console.log("Config: environment variables");
    runnerConfig = await loadSignalRunnerConfig();
  }

  // Validate signals are specified
  if (runnerConfig.signals.length === 0) {
    console.error("No signals specified.");
    console.error("Use --config /path/to/signal.yaml or set SIGNALS env var");
    console.error("Example: SIGNALS=volume-1m-30min,other-signal");
    Deno.exit(1);
  }

  console.log(`Signals: ${runnerConfig.signals.join(", ")}`);
  console.log(`NATS: ${runnerConfig.nats.url}`);
  console.log(`Stream: ${runnerConfig.nats.stream}`);
  if (runnerConfig.nats.filter) {
    console.log(`Filter: ${runnerConfig.nats.filter}`);
  }
  if (runnerConfig.filters?.categories) {
    console.log(`Categories: ${runnerConfig.filters.categories.join(", ")}`);
  }
  if (runnerConfig.filters?.tickers) {
    console.log(`Tickers: ${runnerConfig.filters.tickers.join(", ")}`);
  }
  console.log();

  // Build signal paths
  const signalPaths = runnerConfig.signals.map(name => join(config.signalsPath, name));

  // Verify all signals exist
  for (const path of signalPaths) {
    try {
      await Deno.stat(join(path, "signal.ts"));
    } catch {
      console.error(`Signal not found: ${path}/signal.ts`);
      Deno.exit(1);
    }
  }

  // Create NATS source with filter from config
  const filter = runnerConfig.nats.filter ?? "prod.kalshi.>";
  const source = new NatsRecordSource(
    runnerConfig.nats.url,
    runnerConfig.nats.stream,
    filter
  );

  // Wrap NATS sink with logging
  const natsSink = new NatsFireSink(runnerConfig.nats.url);
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
