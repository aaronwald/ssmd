// scale.ts - Scale SSMD components up/down for maintenance windows

interface ScaleFlags {
  _: (string | number)[];
  env?: string;
  wait?: boolean;
  "dry-run"?: boolean;
}

// Components in scale-down order (operator first to stop reconciliation, then upstream)
const COMPONENTS = [
  { label: "operator", deployment: "ssmd-operator" },
  { label: "connectors", selector: "app.kubernetes.io/name=ssmd-connector" },
  { label: "signals", selector: "app.kubernetes.io/name=ssmd-signal" },
  { label: "notifier", deployment: "ssmd-notifier" },
  { label: "archiver", selector: "app.kubernetes.io/name=ssmd-archiver" },
  { label: "data-api", deployment: "ssmd-data-ts" },
];


export async function handleScale(
  subcommand: string,
  flags: ScaleFlags
): Promise<void> {
  const namespace = "ssmd";
  const dryRun = flags["dry-run"] ?? false;

  switch (subcommand) {
    case "down":
      await scaleDown(namespace, dryRun);
      break;
    case "up":
      await scaleUp(dryRun);
      break;
    case "status":
    case undefined:
      await scaleStatus(namespace);
      break;
    default:
      console.error(`Unknown scale command: ${subcommand}`);
      printScaleHelp();
      Deno.exit(1);
  }
}

async function scaleDown(namespace: string, dryRun: boolean): Promise<void> {
  console.log("Scaling down SSMD components...\n");

  // Suspend Flux kustomization first to prevent reconciliation
  console.log("Suspending Flux kustomization...");
  if (!dryRun) {
    try {
      await flux(["suspend", "kustomization", "ssmd"]);
      console.log("  Flux ssmd kustomization suspended\n");
    } catch (e) {
      console.error(`  Failed to suspend Flux: ${e}`);
      console.error("  Aborting scale down to prevent drift/restore loop");
      Deno.exit(1);
    }
  } else {
    console.log("[dry-run] Would suspend Flux kustomization ssmd\n");
  }

  // Scale down components in order (operator first)
  for (const component of COMPONENTS) {
    if (dryRun) {
      console.log(`[dry-run] Would scale ${component.label} to 0`);
      continue;
    }

    console.log(`Scaling ${component.label} to 0...`);
    try {
      if (component.selector) {
        await kubectl([
          "scale", "deployment",
          "-n", namespace,
          "-l", component.selector,
          "--replicas=0",
        ]);
      } else if (component.deployment) {
        await kubectl([
          "scale", "deployment", component.deployment,
          "-n", namespace,
          "--replicas=0",
        ]);
      }
      console.log(`  ${component.label} scaled to 0`);

      // Wait for operator to fully stop before scaling other components
      if (component.label === "operator") {
        console.log("  Waiting for operator to stop...");
        await new Promise(r => setTimeout(r, 5000));
      }
    } catch (e) {
      console.error(`  Failed to scale ${component.label}: ${e}`);
    }
  }

  // Wait for pods to terminate
  console.log("\nWaiting for pods to terminate...");
  if (!dryRun) {
    await waitForPodsTerminated(namespace, 120);
  }

  console.log("\nScale down complete.");
  console.log("Run 'ssmd archiver sync kalshi-archiver' to sync data to GCS.");
}

async function scaleUp(dryRun: boolean): Promise<void> {
  console.log("Scaling up SSMD components via Flux...\n");

  if (dryRun) {
    console.log("[dry-run] Would resume Flux kustomization ssmd");
    return;
  }

  // Resume Flux kustomization (this triggers reconciliation automatically)
  console.log("Resuming Flux kustomization...");
  try {
    await flux(["resume", "kustomization", "ssmd"]);
    console.log("  Flux ssmd kustomization resumed");
  } catch (e) {
    console.error(`  Failed to resume Flux: ${e}`);
    Deno.exit(1);
  }

  // Force reconcile to restore immediately
  console.log("Triggering reconciliation...");
  await flux(["reconcile", "kustomization", "ssmd", "--with-source"]);

  console.log("\nScale up triggered. Use 'ssmd scale status' to monitor.");
}

async function scaleStatus(namespace: string): Promise<void> {
  console.log("SSMD Component Status\n");
  console.log("COMPONENT                          READY   REPLICAS");
  console.log("---------                          -----   --------");

  for (const component of COMPONENTS) {
    try {
      let jsonOutput: string;
      if (component.selector) {
        jsonOutput = await kubectl([
          "get", "deployment",
          "-n", namespace,
          "-l", component.selector,
          "-o", "json",
        ]);
      } else if (component.deployment) {
        jsonOutput = await kubectl([
          "get", "deployment", component.deployment,
          "-n", namespace,
          "-o", "json",
        ]);
      } else {
        continue;
      }

      const parsed = JSON.parse(jsonOutput);
      const items = parsed.items ?? [parsed]; // Single deployment vs list

      if (items.length === 0) {
        console.log(`${component.label.padEnd(34)} N/A     (not found)`);
        continue;
      }

      for (const dep of items) {
        const name = dep.metadata?.name ?? component.label;
        const ready = dep.status?.readyReplicas ?? 0;
        const total = dep.status?.replicas ?? 0;
        const replicas = `${ready}/${total}`;
        const status = total === 0 ? "SCALED" : (ready === total ? "READY" : "PENDING");
        console.log(`${name.padEnd(34)} ${status.padEnd(7)} ${replicas}`);
      }
    } catch {
      console.log(`${component.label.padEnd(34)} ERROR   (failed to get)`);
    }
  }

}

async function waitForPodsTerminated(namespace: string, timeoutSec: number): Promise<void> {
  const start = Date.now();
  const selectors = COMPONENTS
    .filter(c => c.selector || c.deployment)
    .map(c => c.selector || `app.kubernetes.io/name=${c.deployment}`);

  while (Date.now() - start < timeoutSec * 1000) {
    let allTerminated = true;

    for (const selector of selectors) {
      try {
        const output = await kubectl([
          "get", "pods",
          "-n", namespace,
          "-l", selector,
          "-o", "jsonpath={.items[*].metadata.name}",
        ]);
        if (output.trim()) {
          allTerminated = false;
          break;
        }
      } catch {
        // Ignore errors
      }
    }

    if (allTerminated) {
      console.log("  All pods terminated");
      return;
    }

    await new Promise(r => setTimeout(r, 2000));
  }

  console.log("  Warning: timeout waiting for pods to terminate");
}

// Helper functions

async function kubectl(args: string[]): Promise<string> {
  const cmd = new Deno.Command("kubectl", { args, stdout: "piped", stderr: "piped" });
  const { stdout, stderr, code } = await cmd.output();

  if (code !== 0) {
    const err = new TextDecoder().decode(stderr);
    throw new Error(err.trim());
  }

  return new TextDecoder().decode(stdout);
}

async function flux(args: string[]): Promise<string> {
  const cmd = new Deno.Command("flux", { args, stdout: "piped", stderr: "piped" });
  const { stdout, stderr, code } = await cmd.output();

  if (code !== 0) {
    const err = new TextDecoder().decode(stderr);
    throw new Error(err.trim());
  }

  return new TextDecoder().decode(stdout);
}

function printScaleHelp(): void {
  console.log("Usage: ssmd scale <command> [options]");
  console.log("");
  console.log("Commands:");
  console.log("  down      Suspend Flux + scale all SSMD components to 0");
  console.log("  up        Resume Flux + reconcile (restores git-defined replicas)");
  console.log("  status    Show current component status and replica counts");
  console.log("");
  console.log("Options:");
  console.log("  --dry-run    Show what would be done without making changes");
  console.log("");
  console.log("Examples:");
  console.log("  ssmd scale status");
  console.log("  ssmd scale down --dry-run");
  console.log("  ssmd scale down");
  console.log("  ssmd archiver sync kalshi-archiver   # sync to GCS after scale down");
  console.log("  ssmd scale up");
}
