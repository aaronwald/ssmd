// scale.ts - SSMD component scaling for maintenance windows
// Manages scaling all SSMD components up/down via kubectl and Flux

interface ScaleFlags {
  _: (string | number)[];
  env?: string;
  wait?: boolean;
  namespace?: string;
}

// Components in scale-down order (upstream first)
const COMPONENTS = [
  { name: "connectors", label: "app.kubernetes.io/name=ssmd-connector", type: "label" },
  { name: "signal-runner", deployment: "ssmd-signal-runner", type: "deployment" },
  { name: "notifier", deployment: "ssmd-notifier", type: "deployment" },
  { name: "archiver", label: "app.kubernetes.io/name=ssmd-archiver", type: "label" },
  { name: "data-api", deployment: "ssmd-data-ts", type: "deployment" },
];

// NATS streams to purge (for prod env)
const NATS_STREAMS = [
  "PROD_KALSHI_ECONOMICS",
  "PROD_KALSHI_POLITICS",
  "PROD_KALSHI_SPORTS",
  "PROD_KALSHI_ENTERTAINMENT",
  "SIGNALS",
  "SECMASTER_CDC",
];

export async function handleScale(
  subcommand: string,
  flags: ScaleFlags
): Promise<void> {
  switch (subcommand) {
    case "down":
      await scaleDown(flags);
      break;
    case "up":
      await scaleUp(flags);
      break;
    case "status":
      await scaleStatus(flags);
      break;
    default:
      console.error(`Unknown scale command: ${subcommand}`);
      printScaleHelp();
      Deno.exit(1);
  }
}

async function scaleDown(flags: ScaleFlags): Promise<void> {
  const ns = flags.namespace || "ssmd";
  const env = flags.env || "prod";

  console.log(`Scaling down SSMD components (env: ${env}, namespace: ${ns})...\n`);

  // 0. Suspend Flux reconciliation to prevent restoring deployments
  console.log("Suspending Flux reconciliation for ssmd...");
  try {
    await flux(["suspend", "kustomization", "ssmd", "-n", "flux-system"]);
    console.log("  Flux reconciliation suspended");
  } catch (e) {
    console.error(`  Failed to suspend Flux: ${e}`);
    // Continue anyway - manual scale might still work
  }

  // 1. Scale down components in order (upstream first)
  for (const component of COMPONENTS) {
    console.log(`Scaling down ${component.name}...`);
    try {
      if (component.type === "label") {
        await kubectl([
          "scale", "deployment", "-n", ns,
          "-l", component.label!,
          "--replicas=0"
        ]);
      } else {
        await kubectl([
          "scale", "deployment", component.deployment!,
          "-n", ns,
          "--replicas=0"
        ]);
      }

      // Wait for pods to terminate
      await waitForPodsTerminated(ns, component);
      console.log(`  ${component.name} scaled to 0`);
    } catch (e) {
      console.error(`  Failed to scale ${component.name}: ${e}`);
      // Continue with other components
    }
  }

  // 2. Wait for archiver GCS sync job to complete
  console.log("\nWaiting for archiver GCS sync job...");
  const syncCompleted = await waitForSyncJob(ns, "kalshi-archiver-final-sync", 300000); // 5 min timeout
  if (syncCompleted) {
    console.log("  GCS sync job completed");
  } else {
    console.log("  GCS sync job not found or timed out (may not have been triggered)");
  }

  // 3. Purge NATS streams
  console.log("\nPurging NATS streams...");
  for (const stream of NATS_STREAMS) {
    try {
      await purgeNatsStream(stream);
      console.log(`  Purged ${stream}`);
    } catch (e) {
      console.error(`  Failed to purge ${stream}: ${e}`);
    }
  }

  console.log("\nScale down complete.");
}

async function scaleUp(flags: ScaleFlags): Promise<void> {
  const ns = flags.namespace || "ssmd";
  const env = flags.env || "prod";
  const shouldWait = flags.wait !== false;

  console.log(`Scaling up SSMD components via Flux (env: ${env})...\n`);

  // 0. Resume Flux reconciliation (in case it was suspended)
  console.log("Resuming Flux reconciliation for ssmd...");
  try {
    await flux(["resume", "kustomization", "ssmd", "-n", "flux-system"]);
    console.log("  Flux reconciliation resumed");
  } catch (e) {
    console.error(`  Failed to resume Flux: ${e}`);
    // Continue anyway - it might not have been suspended
  }

  // 1. Reconcile Flux source
  console.log("Reconciling Flux git source...");
  try {
    await flux(["reconcile", "source", "git", "flux-system"]);
    console.log("  Git source reconciled");
  } catch (e) {
    console.error(`  Failed to reconcile git source: ${e}`);
  }

  // 2. Reconcile ssmd kustomization
  console.log("Reconciling ssmd kustomization...");
  try {
    await flux(["reconcile", "kustomization", "ssmd", "--with-source"]);
    console.log("  Kustomization reconciled");
  } catch (e) {
    console.error(`  Failed to reconcile kustomization: ${e}`);
    Deno.exit(1);
  }

  // 3. Wait for deployments to be ready
  if (shouldWait) {
    console.log("\nWaiting for deployments to be ready...");
    // Reverse order for scale up (downstream first)
    const reversed = [...COMPONENTS].reverse();
    for (const component of reversed) {
      try {
        await waitForDeploymentReady(ns, component, 120000); // 2 min per component
        console.log(`  ${component.name} ready`);
      } catch (e) {
        console.error(`  ${component.name} not ready: ${e}`);
      }
    }
  }

  console.log("\nScale up complete.");
}

async function scaleStatus(flags: ScaleFlags): Promise<void> {
  const ns = flags.namespace || "ssmd";

  console.log(`SSMD Component Status (namespace: ${ns})\n`);
  console.log("COMPONENT        REPLICAS  READY     STATUS");
  console.log("---------        --------  -----     ------");

  for (const component of COMPONENTS) {
    try {
      let deployments: string;
      if (component.type === "label") {
        deployments = await kubectl([
          "get", "deployment", "-n", ns,
          "-l", component.label!,
          "-o", "jsonpath={range .items[*]}{.metadata.name} {.spec.replicas} {.status.readyReplicas} {.status.conditions[?(@.type=='Available')].status}\\n{end}"
        ]);
      } else {
        deployments = await kubectl([
          "get", "deployment", component.deployment!,
          "-n", ns,
          "-o", "jsonpath={.metadata.name} {.spec.replicas} {.status.readyReplicas} {.status.conditions[?(@.type=='Available')].status}"
        ]);
      }

      for (const line of deployments.split("\n").filter(Boolean)) {
        const [name, replicas, ready, available] = line.split(" ");
        const status = available === "True" ? "Available" : "NotReady";
        console.log(`${name.padEnd(16)} ${(replicas || "0").padEnd(9)} ${(ready || "0").padEnd(9)} ${status}`);
      }
    } catch (_e) {
      console.log(`${component.name.padEnd(16)} -         -         NotFound`);
    }
  }

  // Show NATS stream message counts
  console.log("\nNATS Streams:");
  console.log("STREAM                         MESSAGES");
  console.log("------                         --------");
  for (const stream of NATS_STREAMS) {
    try {
      const count = await getNatsStreamMessageCount(stream);
      console.log(`${stream.padEnd(30)} ${count}`);
    } catch (_e) {
      console.log(`${stream.padEnd(30)} -`);
    }
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

async function flux(args: string[]): Promise<string> {
  const cmd = new Deno.Command("flux", { args, stdout: "piped", stderr: "piped" });
  const { stdout, stderr, code } = await cmd.output();

  if (code !== 0) {
    const err = new TextDecoder().decode(stderr);
    throw new Error(`flux failed: ${err}`);
  }

  return new TextDecoder().decode(stdout);
}

async function waitForPodsTerminated(
  ns: string,
  component: typeof COMPONENTS[0],
  timeoutMs = 60000
): Promise<void> {
  const start = Date.now();

  while (Date.now() - start < timeoutMs) {
    let podCount: number;

    if (component.type === "label") {
      const output = await kubectl([
        "get", "pods", "-n", ns,
        "-l", component.label!,
        "-o", "jsonpath={.items[*].metadata.name}"
      ]);
      podCount = output.trim() ? output.trim().split(" ").length : 0;
    } else {
      const output = await kubectl([
        "get", "pods", "-n", ns,
        "-l", `app.kubernetes.io/name=${component.deployment}`,
        "-o", "jsonpath={.items[*].metadata.name}"
      ]);
      podCount = output.trim() ? output.trim().split(" ").length : 0;
    }

    if (podCount === 0) {
      return;
    }

    await new Promise(r => setTimeout(r, 2000));
  }

  throw new Error("Timeout waiting for pods to terminate");
}

async function waitForDeploymentReady(
  ns: string,
  component: typeof COMPONENTS[0],
  timeoutMs = 120000
): Promise<void> {
  const start = Date.now();

  while (Date.now() - start < timeoutMs) {
    let ready = false;

    if (component.type === "label") {
      const output = await kubectl([
        "get", "deployment", "-n", ns,
        "-l", component.label!,
        "-o", "jsonpath={range .items[*]}{.status.readyReplicas}/{.spec.replicas} {end}"
      ]);

      // All deployments must have readyReplicas == replicas
      const pairs = output.trim().split(" ").filter(Boolean);
      ready = pairs.length > 0 && pairs.every(p => {
        const [readyStr, replicasStr] = p.split("/");
        const readyCount = parseInt(readyStr) || 0;
        const replicas = parseInt(replicasStr) || 0;
        return replicas > 0 && readyCount >= replicas;
      });
    } else {
      const output = await kubectl([
        "get", "deployment", component.deployment!,
        "-n", ns,
        "-o", "jsonpath={.status.readyReplicas}/{.spec.replicas}"
      ]);
      const [readyStr, replicasStr] = output.split("/");
      const readyCount = parseInt(readyStr) || 0;
      const replicas = parseInt(replicasStr) || 0;
      ready = replicas > 0 && readyCount >= replicas;
    }

    if (ready) {
      return;
    }

    await new Promise(r => setTimeout(r, 2000));
  }

  throw new Error("Timeout waiting for deployment to be ready");
}

async function waitForSyncJob(ns: string, jobName: string, timeoutMs: number): Promise<boolean> {
  const start = Date.now();

  while (Date.now() - start < timeoutMs) {
    try {
      const status = await kubectl([
        "get", "job", jobName, "-n", ns,
        "-o", "jsonpath={.status.succeeded}"
      ]);

      if (status.trim() === "1") {
        return true;
      }
    } catch (_e) {
      // Job might not exist yet
    }

    await new Promise(r => setTimeout(r, 5000));
  }

  return false;
}

async function purgeNatsStream(stream: string): Promise<void> {
  // Find nats-box pod
  const natsBoxPod = await kubectl([
    "get", "pod", "-n", "nats",
    "-l", "app.kubernetes.io/name=nats-box",
    "-o", "jsonpath={.items[0].metadata.name}"
  ]);

  if (!natsBoxPod.trim()) {
    throw new Error("nats-box pod not found");
  }

  await kubectl([
    "exec", "-n", "nats", natsBoxPod.trim(), "--",
    "nats", "stream", "purge", stream, "-f"
  ]);
}

async function getNatsStreamMessageCount(stream: string): Promise<string> {
  // Find nats-box pod
  const natsBoxPod = await kubectl([
    "get", "pod", "-n", "nats",
    "-l", "app.kubernetes.io/name=nats-box",
    "-o", "jsonpath={.items[0].metadata.name}"
  ]);

  if (!natsBoxPod.trim()) {
    return "-";
  }

  const output = await kubectl([
    "exec", "-n", "nats", natsBoxPod.trim(), "--",
    "nats", "stream", "info", stream, "-j"
  ]);

  try {
    const info = JSON.parse(output);
    return info.state?.messages?.toString() || "0";
  } catch (_e) {
    return "-";
  }
}

function printScaleHelp(): void {
  console.log("Usage: ssmd scale <command> [options]");
  console.log("");
  console.log("Commands:");
  console.log("  down      Scale down all SSMD components, sync to GCS, purge NATS");
  console.log("  up        Scale up all SSMD components via Flux reconcile");
  console.log("  status    Show current scale status of all components");
  console.log("");
  console.log("Options:");
  console.log("  --env <env>         Environment (default: prod)");
  console.log("  --namespace <ns>    Kubernetes namespace (default: ssmd)");
  console.log("  --no-wait           Don't wait for ready status on scale up");
  console.log("");
  console.log("Examples:");
  console.log("  ssmd scale down                    # Scale down prod");
  console.log("  ssmd scale up                      # Scale up via Flux");
  console.log("  ssmd scale status                  # Show component status");
  console.log("  ssmd scale down --env staging      # Scale down staging");
}
