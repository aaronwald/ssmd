// signal-deploy.ts - Signal CR deployment management
// Manages Signal CRs via kubectl (kubernetes deployment lifecycle)

interface SignalDeployFlags {
  _: (string | number)[];
  follow?: boolean;
  tail?: string;
  namespace?: string;
}

const DEFAULT_NAMESPACE = "ssmd";

export async function handleSignalDeploy(
  subcommand: string,
  flags: SignalDeployFlags
): Promise<void> {
  const ns = flags.namespace ?? DEFAULT_NAMESPACE;

  switch (subcommand) {
    case "deploy":
      await deploySignal(flags, ns);
      break;
    case "list":
      await listSignals(ns);
      break;
    case "status":
      await statusSignal(flags, ns);
      break;
    case "logs":
      await logsSignal(flags, ns);
      break;
    case "delete":
      await deleteSignal(flags, ns);
      break;
    default:
      console.error(`Unknown signal-deploy command: ${subcommand}`);
      printSignalDeployHelp();
      Deno.exit(1);
  }
}

async function deploySignal(flags: SignalDeployFlags, ns: string): Promise<void> {
  const file = flags._[2] as string;

  if (!file) {
    console.error("Usage: ssmd signal deploy <file.yaml>");
    Deno.exit(1);
  }

  // Check if file exists
  try {
    await Deno.stat(file);
  } catch {
    console.error(`File not found: ${file}`);
    Deno.exit(1);
  }

  console.log(`Deploying Signal from ${file}...`);

  try {
    const output = await kubectl(["apply", "-f", file, "-n", ns]);
    console.log(output.trim());
  } catch (e) {
    console.error(`Failed to deploy: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function listSignals(ns: string): Promise<void> {
  console.log("Signal CRs:\n");
  console.log("NAME".padEnd(25) + "SIGNALS".padEnd(30) + "STREAM".padEnd(20) + "PHASE".padEnd(12) + "AGE");
  console.log("----".padEnd(25) + "-------".padEnd(30) + "------".padEnd(20) + "-----".padEnd(12) + "---");

  try {
    // Get signals with all needed fields
    const signals = await kubectl([
      "get", "signal", "-n", ns,
      "-o", "jsonpath={range .items[*]}{.metadata.name}|{.spec.signals}|{.spec.source.stream}|{.status.phase}|{.metadata.creationTimestamp}\\n{end}"
    ]).catch(() => "");

    if (!signals.trim()) {
      console.log("(no signals found)");
      return;
    }

    for (const line of signals.split("\n").filter(Boolean)) {
      const [name, signalsJson, stream, phase, createdAt] = line.split("|");

      // Parse signals array - kubectl returns it as JSON array format like ["sig1","sig2"]
      let signalsStr = "-";
      if (signalsJson) {
        try {
          const signalsArray = JSON.parse(signalsJson);
          signalsStr = Array.isArray(signalsArray) ? signalsArray.join(",") : signalsJson;
        } catch {
          signalsStr = signalsJson.replace(/[\[\]"]/g, "");
        }
      }

      const age = createdAt ? formatAge(createdAt) : "-";

      console.log(
        (name || "-").padEnd(25) +
        (signalsStr || "-").padEnd(30) +
        (stream || "-").padEnd(20) +
        (phase || "-").padEnd(12) +
        age
      );
    }
  } catch (e) {
    console.error(`Failed to list signals: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function statusSignal(flags: SignalDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd signal status <name>");
    Deno.exit(1);
  }

  try {
    // Get full Signal CR as JSON
    const signalJson = await kubectl([
      "get", "signal", name, "-n", ns, "-o", "json"
    ]);

    const signal = JSON.parse(signalJson);

    console.log(`Signal: ${signal.metadata.name}`);
    console.log(`Namespace: ${signal.metadata.namespace}`);
    console.log(`Created: ${signal.metadata.creationTimestamp} (${formatAge(signal.metadata.creationTimestamp)})`);
    console.log();

    // Spec
    console.log("Spec:");
    console.log(`  Signals: ${signal.spec.signals?.join(", ") || "-"}`);
    console.log(`  Image: ${signal.spec.image || "-"}`);
    console.log(`  Output Prefix: ${signal.spec.outputPrefix || "signals"}`);
    console.log();

    console.log("  Source:");
    console.log(`    Stream: ${signal.spec.source?.stream || "-"}`);
    if (signal.spec.source?.natsUrl) {
      console.log(`    NATS URL: ${signal.spec.source.natsUrl}`);
    }
    if (signal.spec.source?.categories?.length) {
      console.log(`    Categories: ${signal.spec.source.categories.join(", ")}`);
    }
    if (signal.spec.source?.tickers?.length) {
      console.log(`    Tickers: ${signal.spec.source.tickers.join(", ")}`);
    }
    console.log();

    // Status
    console.log("Status:");
    console.log(`  Phase: ${signal.status?.phase || "Unknown"}`);
    if (signal.status?.deployment) {
      console.log(`  Deployment: ${signal.status.deployment}`);
    }
    console.log();

    // Signal Metrics
    if (signal.status?.signalMetrics?.length) {
      console.log("Signal Metrics:");
      for (const metric of signal.status.signalMetrics) {
        console.log(`  ${metric.signal}:`);
        console.log(`    Messages Processed: ${(metric.messagesProcessed || 0).toLocaleString()}`);
        console.log(`    Fires Emitted: ${metric.firesEmitted || 0}`);
        if (metric.lastFireAt) {
          console.log(`    Last Fire: ${metric.lastFireAt} (${formatAge(metric.lastFireAt)})`);
        }
      }
      console.log();
    }

    // Conditions
    if (signal.status?.conditions?.length) {
      console.log("Conditions:");
      for (const cond of signal.status.conditions) {
        const status = cond.status === "True" ? "+" : "-";
        console.log(`  [${status}] ${cond.type}: ${cond.reason}`);
        if (cond.message) {
          console.log(`      ${cond.message}`);
        }
      }
    }
  } catch (e) {
    console.error(`Failed to get signal status: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function logsSignal(flags: SignalDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd signal logs <name> [--follow] [--tail N]");
    Deno.exit(1);
  }

  try {
    // First get the deployment name from the Signal CR
    const deploymentName = await kubectl([
      "get", "signal", name, "-n", ns,
      "-o", "jsonpath={.status.deployment}"
    ]).catch(() => "");

    // If no deployment in status, try the conventional name
    const targetDeployment = deploymentName.trim() || `signal-${name}`;

    // Build kubectl logs args
    const logsArgs = ["logs", "-n", ns, `deployment/${targetDeployment}`];

    if (flags.follow) {
      logsArgs.push("-f");
    }

    if (flags.tail !== undefined) {
      logsArgs.push("--tail", String(flags.tail));
    }

    // Stream logs directly to stdout/stderr
    await kubectlStream(logsArgs);
  } catch (e) {
    console.error(`Failed to get logs: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function deleteSignal(flags: SignalDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd signal delete <name>");
    Deno.exit(1);
  }

  console.log(`Deleting Signal ${name}...`);

  try {
    await kubectl(["delete", "signal", name, "-n", ns]);
    console.log(`Signal ${name} deleted`);
  } catch (e) {
    console.error(`Failed to delete signal: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

// Helper functions

async function kubectl(args: string[]): Promise<string> {
  const cmd = new Deno.Command("kubectl", { args, stdout: "piped", stderr: "piped" });
  const { stdout, stderr, code } = await cmd.output();

  if (code !== 0) {
    const err = new TextDecoder().decode(stderr);
    throw new Error(`kubectl failed: ${err}`);
  }

  return new TextDecoder().decode(stdout);
}

async function kubectlStream(args: string[]): Promise<void> {
  const cmd = new Deno.Command("kubectl", {
    args,
    stdout: "inherit",
    stderr: "inherit",
  });

  const { code } = await cmd.output();

  if (code !== 0) {
    throw new Error(`kubectl logs failed with code ${code}`);
  }
}

function formatAge(timestamp: string): string {
  const created = new Date(timestamp);
  const now = new Date();
  const diffMs = now.getTime() - created.getTime();

  const seconds = Math.floor(diffMs / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);

  if (days > 0) {
    return `${days}d`;
  } else if (hours > 0) {
    return `${hours}h`;
  } else if (minutes > 0) {
    return `${minutes}m`;
  } else {
    return `${seconds}s`;
  }
}

export function printSignalDeployHelp(): void {
  console.log("Usage: ssmd signal <deploy-command> [options]");
  console.log();
  console.log("Kubernetes Signal CR Management Commands:");
  console.log("  deploy <file.yaml>     Deploy a Signal CR from YAML file");
  console.log("  list                   List all Signal CRs");
  console.log("  status <name>          Show detailed Signal status");
  console.log("  logs <name>            Show logs from Signal pod");
  console.log("  delete <name>          Delete a Signal CR");
  console.log();
  console.log("Options:");
  console.log("  --namespace NS         Kubernetes namespace (default: ssmd)");
  console.log("  --follow, -f           Follow log output (logs command)");
  console.log("  --tail N               Number of lines to show (logs command)");
  console.log();
  console.log("Examples:");
  console.log("  ssmd signal deploy signals/my-signal/signal.yaml");
  console.log("  ssmd signal list");
  console.log("  ssmd signal status my-signal");
  console.log("  ssmd signal logs my-signal --follow --tail 100");
  console.log("  ssmd signal delete my-signal");
}
