// ssmd-agent/src/cli/commands/signal.ts
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { connect, StringCodec } from "npm:nats";
import { runSignal } from "../../runtime/runner.ts";
import { NatsRecordSource, NatsFireSink, ConsoleFireSink } from "../../runtime/nats.ts";
import { FileRecordSource, getTodayDate } from "../../runtime/file.ts";
import type { RecordSource, FireSink, SignalFire } from "../../runtime/interfaces.ts";
import { loadSignal } from "../../backtest/loader.ts";
import { config } from "../../config.ts";

const sc = StringCodec();

/**
 * List available signals
 */
async function listSignals(): Promise<void> {
  const signalsDir = "signals";

  console.log("Available signals:");
  console.log();

  try {
    for await (const entry of Deno.readDir(signalsDir)) {
      if (entry.isDirectory) {
        try {
          const signalPath = join(signalsDir, entry.name);
          const signal = await loadSignal(signalPath);
          console.log(`  ${entry.name.padEnd(25)} ${signal.name ?? signal.id}`);
        } catch {
          // Not a valid signal directory
        }
      }
    }
  } catch {
    console.error("No signals directory found");
  }
}

/**
 * Run a signal against a data source
 */
async function runSignalCommand(
  signalName: string,
  flags: Record<string, unknown>
): Promise<void> {
  const signalPath = join("signals", signalName);

  // Check signal exists
  try {
    await Deno.stat(join(signalPath, "signal.ts"));
  } catch {
    console.error(`Signal not found: ${signalPath}/signal.ts`);
    await listSignals();
    Deno.exit(1);
  }

  // Determine source type
  const sourceType = (flags.source as string) ?? "nats";

  let source: RecordSource;
  let sink: FireSink;

  if (sourceType === "file") {
    // File source
    const dataDir = (flags.data as string) ?? "data";
    const feed = (flags.feed as string) ?? "kalshi";
    const dates = flags.dates
      ? (flags.dates as string).split(",")
      : [getTodayDate()];

    source = new FileRecordSource(dataDir, feed, dates);
    sink = new ConsoleFireSink();

    console.log(`Source: file (${dataDir}/${feed})`);
    console.log(`Dates: ${dates.join(", ")}`);
  } else {
    // NATS source
    const natsUrl = (flags["nats-url"] as string) ?? config.natsUrl ?? "nats://localhost:4222";
    const stream = (flags.stream as string) ?? "PROD_KALSHI";
    const subject = (flags.subject as string) ?? "prod.kalshi.>";

    source = new NatsRecordSource(natsUrl, stream, subject);

    // Use NATS sink unless --console flag
    if (flags.console) {
      sink = new ConsoleFireSink();
    } else {
      sink = new NatsFireSink(natsUrl);
    }

    console.log(`Source: NATS (${natsUrl})`);
    console.log(`Stream: ${stream}`);
  }

  console.log();

  // Load signal manifest for state config
  const signal = await loadSignal(signalPath);
  const stateConfig = signal.manifest?.state as Record<string, Record<string, unknown>> | undefined;

  // Run the signal
  const stats = await runSignal({
    signalPath,
    source,
    sink,
    stateConfig,
  });

  // Print final stats
  console.log();
  console.log("=== Runtime Complete ===");
  console.log(`Records: ${stats.recordsProcessed.toLocaleString()}`);
  console.log(`Tickers: ${stats.tickersTracked.toLocaleString()}`);
  console.log(`Fires: ${stats.firesPublished}`);
  console.log(`Errors: ${stats.errors}`);
  console.log(`Duration: ${((Date.now() - stats.startTime) / 1000).toFixed(1)}s`);
}

/**
 * Subscribe to a signal's fire stream
 */
async function subscribeCommand(
  signalName: string,
  flags: Record<string, unknown>
): Promise<void> {
  const natsUrl = (flags["nats-url"] as string) ?? config.natsUrl ?? "nats://localhost:4222";
  const subject = `signals.${signalName}.fires`;

  console.log(`Connecting to NATS: ${natsUrl}`);
  console.log(`Subscribing to: ${subject}`);
  console.log();

  const nc = await connect({ servers: natsUrl });
  const sub = nc.subscribe(subject);

  console.log("Waiting for fires... (Ctrl+C to exit)");
  console.log();

  for await (const msg of sub) {
    try {
      const fire: SignalFire = JSON.parse(sc.decode(msg.data));
      const time = new Date(fire.ts * 1000).toISOString();
      console.log(`${time} ${fire.ticker}`);
      console.log(`  ${JSON.stringify(fire.payload)}`);
    } catch (e) {
      console.error(`Failed to parse fire: ${e}`);
    }
  }
}

/**
 * Handle signal subcommands
 */
export async function handleSignal(
  subcommand: string,
  flags: Record<string, unknown>
): Promise<void> {
  switch (subcommand) {
    case "list":
      await listSignals();
      break;

    case "run": {
      const signalName = (flags._ as string[])[2];
      if (!signalName) {
        console.error("Usage: ssmd signal run <signal-name> [options]");
        console.error();
        console.error("Options:");
        console.error("  --source file|nats     Data source (default: nats)");
        console.error("  --data DIR             Data directory for file source");
        console.error("  --dates X,Y,Z          Dates to process for file source");
        console.error("  --feed NAME            Feed name (default: kalshi)");
        console.error("  --nats-url URL         NATS server URL");
        console.error("  --stream NAME          NATS stream name (default: PROD_KALSHI)");
        console.error("  --console              Log fires to console instead of NATS");
        Deno.exit(1);
      }
      await runSignalCommand(signalName, flags);
      break;
    }

    case "subscribe":
    case "sub": {
      const signalName = (flags._ as string[])[2];
      if (!signalName) {
        console.error("Usage: ssmd signal subscribe <signal-name> [options]");
        console.error();
        console.error("Options:");
        console.error("  --nats-url URL         NATS server URL");
        Deno.exit(1);
      }
      await subscribeCommand(signalName, flags);
      break;
    }

    default:
      console.log("Usage: ssmd signal <command>");
      console.log();
      console.log("Commands:");
      console.log("  list                   List available signals");
      console.log("  run <name>             Run a signal against data source");
      console.log("  subscribe <name>       Subscribe to signal fire stream");
      Deno.exit(1);
  }
}
