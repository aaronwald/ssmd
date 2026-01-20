// status.ts - Top-level cluster status overview
// Aggregates status from connectors, archivers, signals, and NATS streams

import { kubectl, getCurrentEnvDisplay, type KubectlOptions } from "../utils/kubectl.ts";
import { getEnvContext } from "../utils/env-context.ts";

interface StatusFlags {
  _: (string | number)[];
  namespace?: string;
  env?: string;
}

export async function handleStatus(flags: StatusFlags): Promise<void> {
  const opts: KubectlOptions = {
    env: flags.env,
    namespace: flags.namespace,
  };

  const envDisplay = await getCurrentEnvDisplay(opts.env);
  const context = await getEnvContext(opts.env);

  console.log(`SSMD Cluster Status (${envDisplay})`);
  console.log("=".repeat(40 + envDisplay.length) + "\n");

  // Run all status checks in parallel
  const [connectors, archivers, signals, streams] = await Promise.all([
    getConnectors(opts),
    getArchivers(opts),
    getSignals(opts),
    getNatsStreams(context.cluster),
  ]);

  // Connectors
  console.log("Connectors:");
  if (connectors.length === 0) {
    console.log("  (none)");
  } else {
    for (const c of connectors) {
      const status = c.phase === "Running" ? "✓" : "✗";
      const msgs = c.messages > 0 ? ` [${formatNumber(c.messages)} msgs]` : "";
      console.log(`  ${status} ${c.name.padEnd(25)} ${c.phase.padEnd(10)}${msgs}`);
    }
  }
  console.log();

  // Archivers
  console.log("Archivers:");
  if (archivers.length === 0) {
    console.log("  (none)");
  } else {
    for (const a of archivers) {
      const status = a.phase === "Running" ? "✓" : "✗";
      const msgs = a.messages > 0 ? ` [${formatNumber(a.messages)} msgs]` : "";
      console.log(`  ${status} ${a.name.padEnd(25)} ${a.phase.padEnd(10)}${msgs}`);
    }
  }
  console.log();

  // Signals
  console.log("Signals:");
  if (signals.length === 0) {
    console.log("  (none)");
  } else {
    for (const s of signals) {
      const status = s.phase === "Running" ? "✓" : "✗";
      console.log(`  ${status} ${s.name.padEnd(25)} ${s.phase}`);
    }
  }
  console.log();

  // NATS Streams
  console.log("NATS Streams:");
  if (streams.length === 0) {
    console.log("  (none or unavailable)");
  } else {
    for (const st of streams) {
      const msgs = formatNumber(st.messages);
      const bytes = formatBytes(st.bytes);
      console.log(`  ${st.name.padEnd(25)} ${msgs.padStart(10)} msgs  ${bytes.padStart(10)}`);
    }
  }
  console.log();

  // Summary
  const runningConnectors = connectors.filter(c => c.phase === "Running").length;
  const runningArchivers = archivers.filter(a => a.phase === "Running").length;
  const runningSignals = signals.filter(s => s.phase === "Running").length;

  console.log("Summary:");
  console.log(`  Connectors: ${runningConnectors}/${connectors.length} running`);
  console.log(`  Archivers:  ${runningArchivers}/${archivers.length} running`);
  console.log(`  Signals:    ${runningSignals}/${signals.length} running`);
  console.log(`  Streams:    ${streams.length} active`);
}

interface ConnectorStatus {
  name: string;
  phase: string;
  messages: number;
}

interface ArchiverStatus {
  name: string;
  phase: string;
  messages: number;
}

interface SignalStatus {
  name: string;
  phase: string;
}

interface StreamStatus {
  name: string;
  messages: number;
  bytes: number;
}

async function getConnectors(opts: KubectlOptions): Promise<ConnectorStatus[]> {
  try {
    const output = await kubectl([
      "get", "connector", "-o", "json"
    ], opts);
    const data = JSON.parse(output);
    return (data.items || []).map((c: Record<string, unknown>) => ({
      name: (c.metadata as Record<string, string>)?.name || "-",
      phase: (c.status as Record<string, unknown>)?.phase as string || "Unknown",
      messages: (c.status as Record<string, unknown>)?.messagesPublished as number || 0,
    }));
  } catch {
    return [];
  }
}

async function getArchivers(opts: KubectlOptions): Promise<ArchiverStatus[]> {
  try {
    const output = await kubectl([
      "get", "archiver", "-o", "json"
    ], opts);
    const data = JSON.parse(output);
    return (data.items || []).map((a: Record<string, unknown>) => ({
      name: (a.metadata as Record<string, string>)?.name || "-",
      phase: (a.status as Record<string, unknown>)?.phase as string || "Unknown",
      messages: (a.status as Record<string, unknown>)?.messagesArchived as number || 0,
    }));
  } catch {
    return [];
  }
}

async function getSignals(opts: KubectlOptions): Promise<SignalStatus[]> {
  try {
    const output = await kubectl([
      "get", "signal", "-o", "json"
    ], opts);
    const data = JSON.parse(output);
    return (data.items || []).map((s: Record<string, unknown>) => ({
      name: (s.metadata as Record<string, string>)?.name || "-",
      phase: (s.status as Record<string, unknown>)?.phase as string || "Unknown",
    }));
  } catch {
    return [];
  }
}

async function getNatsStreams(cluster: string): Promise<StreamStatus[]> {
  // NATS namespace is typically 'nats' in both environments
  const natsNamespace = "nats";

  try {
    // Get stream names first
    const cmd = new Deno.Command("kubectl", {
      args: [
        "--context", cluster,
        "exec", "-n", natsNamespace, "deploy/nats-box", "--",
        "nats", "stream", "ls", "--names", "-s", "nats://nats:4222"
      ],
      stdout: "piped",
      stderr: "piped",
    });
    const { stdout, code } = await cmd.output();

    if (code !== 0) {
      return [];
    }

    const namesOutput = new TextDecoder().decode(stdout);
    const names = namesOutput.trim().split("\n").filter(n => n.trim());

    if (names.length === 0) {
      return [];
    }

    // Get info for each stream in parallel
    const streamPromises = names.map(async (name) => {
      try {
        const infoCmd = new Deno.Command("kubectl", {
          args: [
            "--context", cluster,
            "exec", "-n", natsNamespace, "deploy/nats-box", "--",
            "nats", "stream", "info", name, "--json", "-s", "nats://nats:4222"
          ],
          stdout: "piped",
          stderr: "piped",
        });
        const { stdout: infoStdout, code: infoCode } = await infoCmd.output();

        if (infoCode !== 0) {
          return { name, messages: 0, bytes: 0 };
        }

        const infoOutput = new TextDecoder().decode(infoStdout);
        const data = JSON.parse(infoOutput);
        return {
          name,
          messages: data.state?.messages || 0,
          bytes: data.state?.bytes || 0,
        };
      } catch {
        return { name, messages: 0, bytes: 0 };
      }
    });

    return await Promise.all(streamPromises);
  } catch {
    return [];
  }
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) {
    return `${(n / 1_000_000).toFixed(1)}M`;
  } else if (n >= 1_000) {
    return `${(n / 1_000).toFixed(1)}K`;
  }
  return String(n);
}

function formatBytes(bytes: number): string {
  if (bytes >= 1_073_741_824) {
    return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  } else if (bytes >= 1_048_576) {
    return `${(bytes / 1_048_576).toFixed(1)} MB`;
  } else if (bytes >= 1_024) {
    return `${(bytes / 1_024).toFixed(1)} KB`;
  }
  return `${bytes} B`;
}

export function printStatusHelp(): void {
  console.log("Usage: ssmd [--env <env>] status [options]");
  console.log();
  console.log("Shows overview of SSMD cluster components:");
  console.log("  - Connectors (market data feeds)");
  console.log("  - Archivers (data persistence)");
  console.log("  - Signals (event detection)");
  console.log("  - NATS Streams (message queues)");
  console.log();
  console.log("Options:");
  console.log("  --env <env>       Target environment (default: current from 'ssmd env')");
  console.log("  --namespace NS    Override namespace (default: from environment)");
}
