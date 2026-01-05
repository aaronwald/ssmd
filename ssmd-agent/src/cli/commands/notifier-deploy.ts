// notifier-deploy.ts - Notifier CR deployment management
// Manages Notifier CRs via kubectl (kubernetes deployment lifecycle)

interface NotifierDeployFlags {
  _: (string | number)[];
  follow?: boolean;
  tail?: number;
  namespace?: string;
  message?: string;
  destination?: string;
}

const DEFAULT_NAMESPACE = "ssmd";

export async function handleNotifierDeploy(
  subcommand: string,
  flags: NotifierDeployFlags
): Promise<void> {
  const ns = flags.namespace ?? DEFAULT_NAMESPACE;

  switch (subcommand) {
    case "deploy":
      await deployNotifier(flags, ns);
      break;
    case "list":
      await listNotifiers(ns);
      break;
    case "status":
      await statusNotifier(flags, ns);
      break;
    case "logs":
      await logsNotifier(flags, ns);
      break;
    case "test":
      await testNotifier(flags, ns);
      break;
    case "delete":
      await deleteNotifier(flags, ns);
      break;
    default:
      console.error(`Unknown notifier-deploy command: ${subcommand}`);
      printNotifierHelp();
      Deno.exit(1);
  }
}

async function deployNotifier(flags: NotifierDeployFlags, ns: string): Promise<void> {
  const file = flags._[2] as string;

  if (!file) {
    console.error("Usage: ssmd notifier deploy <file.yaml>");
    Deno.exit(1);
  }

  // Check if file exists
  try {
    await Deno.stat(file);
  } catch {
    console.error(`File not found: ${file}`);
    Deno.exit(1);
  }

  console.log(`Deploying Notifier from ${file}...`);

  try {
    const output = await kubectl(["apply", "-f", file, "-n", ns]);
    console.log(output.trim());
  } catch (e) {
    console.error(`Failed to deploy: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function listNotifiers(ns: string): Promise<void> {
  console.log("Notifier CRs:\n");
  console.log("NAME".padEnd(25) + "DESTINATIONS".padEnd(30) + "PHASE".padEnd(12) + "FIRES".padEnd(10) + "AGE");
  console.log("----".padEnd(25) + "------------".padEnd(30) + "-----".padEnd(12) + "-----".padEnd(10) + "---");

  try {
    // Get notifiers with all needed fields
    const notifiers = await kubectl([
      "get", "notifier", "-n", ns,
      "-o", "jsonpath={range .items[*]}{.metadata.name}|{.spec.destinations}|{.status.phase}|{.status.metrics.firesProcessed}|{.metadata.creationTimestamp}\\n{end}"
    ]).catch(() => "");

    if (!notifiers.trim()) {
      console.log("(no notifiers found)");
      return;
    }

    for (const line of notifiers.split("\n").filter(Boolean)) {
      const [name, destinationsJson, phase, fires, createdAt] = line.split("|");

      // Parse destinations array - extract type from each destination
      let destinationsStr = "-";
      if (destinationsJson) {
        try {
          const destinationsArray = JSON.parse(destinationsJson);
          if (Array.isArray(destinationsArray)) {
            destinationsStr = destinationsArray.map((d: { type?: string }) => d.type || "unknown").join(",");
          }
        } catch {
          // Fallback: try to extract types from the raw string
          destinationsStr = destinationsJson.replace(/[\[\]"{}]/g, "").substring(0, 25);
        }
      }

      const age = createdAt ? formatAge(createdAt) : "-";

      console.log(
        (name || "-").padEnd(25) +
        (destinationsStr || "-").padEnd(30) +
        (phase || "-").padEnd(12) +
        (fires || "0").padEnd(10) +
        age
      );
    }
  } catch (e) {
    console.error(`Failed to list notifiers: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function statusNotifier(flags: NotifierDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd notifier status <name>");
    Deno.exit(1);
  }

  try {
    // Get full Notifier CR as JSON
    const notifierJson = await kubectl([
      "get", "notifier", name, "-n", ns, "-o", "json"
    ]);

    const notifier = JSON.parse(notifierJson);

    console.log(`Notifier: ${notifier.metadata.name}`);
    console.log(`Namespace: ${notifier.metadata.namespace}`);
    console.log(`Created: ${notifier.metadata.creationTimestamp} (${formatAge(notifier.metadata.creationTimestamp)})`);
    console.log();

    // Spec
    console.log("Spec:");
    console.log(`  Image: ${notifier.spec.image || "-"}`);
    if (notifier.spec.source?.subject) {
      console.log(`  Source Subject: ${notifier.spec.source.subject}`);
    }
    if (notifier.spec.source?.natsUrl) {
      console.log(`  NATS URL: ${notifier.spec.source.natsUrl}`);
    }
    console.log();

    // Destinations
    if (notifier.spec.destinations?.length) {
      console.log("Destinations:");
      for (const dest of notifier.spec.destinations) {
        console.log(`  - Type: ${dest.type}`);
        if (dest.name) {
          console.log(`    Name: ${dest.name}`);
        }
        if (dest.webhook) {
          console.log(`    Webhook: ${dest.webhook.substring(0, 50)}...`);
        }
        if (dest.channel) {
          console.log(`    Channel: ${dest.channel}`);
        }
        if (dest.secretRef) {
          console.log(`    Secret: ${dest.secretRef}`);
        }
      }
      console.log();
    }

    // Status
    console.log("Status:");
    console.log(`  Phase: ${notifier.status?.phase || "Unknown"}`);
    if (notifier.status?.deployment) {
      console.log(`  Deployment: ${notifier.status.deployment}`);
    }
    console.log();

    // Metrics
    if (notifier.status?.metrics) {
      console.log("Metrics:");
      console.log(`  Fires Processed: ${(notifier.status.metrics.firesProcessed || 0).toLocaleString()}`);
      console.log(`  Notifications Sent: ${(notifier.status.metrics.notificationsSent || 0).toLocaleString()}`);
      console.log(`  Errors: ${notifier.status.metrics.errors || 0}`);
      if (notifier.status.metrics.lastNotificationAt) {
        console.log(`  Last Notification: ${notifier.status.metrics.lastNotificationAt} (${formatAge(notifier.status.metrics.lastNotificationAt)})`);
      }
      console.log();
    }

    // Destination Metrics
    if (notifier.status?.destinationMetrics?.length) {
      console.log("Destination Metrics:");
      for (const metric of notifier.status.destinationMetrics) {
        console.log(`  ${metric.destination}:`);
        console.log(`    Sent: ${(metric.sent || 0).toLocaleString()}`);
        console.log(`    Errors: ${metric.errors || 0}`);
        if (metric.lastSentAt) {
          console.log(`    Last Sent: ${metric.lastSentAt} (${formatAge(metric.lastSentAt)})`);
        }
      }
      console.log();
    }

    // Conditions
    if (notifier.status?.conditions?.length) {
      console.log("Conditions:");
      for (const cond of notifier.status.conditions) {
        const status = cond.status === "True" ? "+" : "-";
        console.log(`  [${status}] ${cond.type}: ${cond.reason}`);
        if (cond.message) {
          console.log(`      ${cond.message}`);
        }
      }
    }
  } catch (e) {
    console.error(`Failed to get notifier status: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function logsNotifier(flags: NotifierDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd notifier logs <name> [--follow] [--tail N]");
    Deno.exit(1);
  }

  try {
    // First get the deployment name from the Notifier CR
    const deploymentName = await kubectl([
      "get", "notifier", name, "-n", ns,
      "-o", "jsonpath={.status.deployment}"
    ]).catch(() => "");

    // If no deployment in status, try the conventional name
    const targetDeployment = deploymentName.trim() || `notifier-${name}`;

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

async function testNotifier(flags: NotifierDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd notifier test <name> [--message MSG] [--destination DEST]");
    Deno.exit(1);
  }

  try {
    // Get the notifier's source subject
    const notifierJson = await kubectl([
      "get", "notifier", name, "-n", ns, "-o", "json"
    ]);

    const notifier = JSON.parse(notifierJson);
    const sourceSubject = notifier.spec.source?.subject || "signals.>";

    // Build test fire message
    const testMessage = flags.message || "Test notification from CLI";
    const testFire = {
      signal: "cli-test",
      ticker: "TEST",
      message: testMessage,
      timestamp: new Date().toISOString(),
      test: true,
      destination: flags.destination,
    };

    // Determine the subject to publish to (use a test-friendly subject)
    const publishSubject = sourceSubject.replace(">", "cli-test");

    console.log(`Sending test fire to ${publishSubject}...`);
    console.log(`Message: ${testMessage}`);
    if (flags.destination) {
      console.log(`Target destination: ${flags.destination}`);
    }
    console.log();

    // Try to use nats CLI to publish
    const natsAvailable = await checkNatsCliAvailable();

    if (natsAvailable) {
      try {
        const natsUrl = notifier.spec.source?.natsUrl || "nats://nats.nats.svc:4222";
        await natsPublish(natsUrl, publishSubject, JSON.stringify(testFire));
        console.log("Test fire sent successfully via nats CLI");
        console.log();
        console.log("Check notifier logs to verify delivery:");
        console.log(`  ssmd notifier logs ${name} --tail 20`);
      } catch (e) {
        console.error(`Failed to publish via nats CLI: ${e instanceof Error ? e.message : e}`);
        Deno.exit(1);
      }
    } else {
      console.log("Note: nats CLI not available, cannot send test fire directly.");
      console.log();
      console.log("To test manually, use nats CLI or port-forward to NATS:");
      console.log(`  kubectl port-forward -n nats svc/nats 4222:4222`);
      console.log(`  nats pub "${publishSubject}" '${JSON.stringify(testFire)}'`);
      console.log();
      console.log("Or trigger a real signal fire that this notifier subscribes to.");
    }
  } catch (e) {
    console.error(`Failed to test notifier: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function deleteNotifier(flags: NotifierDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd notifier delete <name>");
    Deno.exit(1);
  }

  console.log(`Deleting Notifier ${name}...`);

  try {
    await kubectl(["delete", "notifier", name, "-n", ns]);
    console.log(`Notifier ${name} deleted`);
  } catch (e) {
    console.error(`Failed to delete notifier: ${e instanceof Error ? e.message : e}`);
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

async function checkNatsCliAvailable(): Promise<boolean> {
  try {
    const cmd = new Deno.Command("nats", { args: ["--version"], stdout: "piped", stderr: "piped" });
    const { code } = await cmd.output();
    return code === 0;
  } catch {
    return false;
  }
}

async function natsPublish(natsUrl: string, subject: string, message: string): Promise<void> {
  const cmd = new Deno.Command("nats", {
    args: ["pub", "-s", natsUrl, subject, message],
    stdout: "piped",
    stderr: "piped",
  });

  const { stdout, stderr, code } = await cmd.output();

  if (code !== 0) {
    const err = new TextDecoder().decode(stderr);
    throw new Error(`nats pub failed: ${err}`);
  }

  const out = new TextDecoder().decode(stdout);
  if (out.trim()) {
    console.log(out.trim());
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

export function printNotifierHelp(): void {
  console.log("Usage: ssmd notifier <deploy-command> [options]");
  console.log();
  console.log("Kubernetes Notifier CR Management Commands:");
  console.log("  deploy <file.yaml>     Deploy a Notifier CR from YAML file");
  console.log("  list                   List all Notifier CRs");
  console.log("  status <name>          Show detailed Notifier status");
  console.log("  logs <name>            Show logs from Notifier pod");
  console.log("  test <name>            Send a test notification");
  console.log("  delete <name>          Delete a Notifier CR");
  console.log();
  console.log("Options:");
  console.log("  --namespace NS         Kubernetes namespace (default: ssmd)");
  console.log("  --follow, -f           Follow log output (logs command)");
  console.log("  --tail N               Number of lines to show (logs command)");
  console.log("  --message MSG          Test message content (test command)");
  console.log("  --destination DEST     Target specific destination (test command)");
  console.log();
  console.log("Examples:");
  console.log("  ssmd notifier deploy notifiers/my-notifier/notifier.yaml");
  console.log("  ssmd notifier list");
  console.log("  ssmd notifier status my-notifier");
  console.log("  ssmd notifier logs my-notifier --follow --tail 100");
  console.log("  ssmd notifier test my-notifier --message 'Hello world'");
  console.log("  ssmd notifier test my-notifier --destination slack");
  console.log("  ssmd notifier delete my-notifier");
}
