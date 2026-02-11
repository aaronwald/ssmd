// scale.ts - Scale SSMD components up/down for maintenance windows

import { kubectl, flux, getCurrentEnvDisplay, type KubectlOptions } from "../utils/kubectl.ts";
import { getEnvContext } from "../utils/env-context.ts";

interface ScaleFlags {
  _: (string | number)[];
  env?: string;
  wait?: boolean;
  "dry-run"?: boolean;
}

// Components in scale-down order:
// 1. Operator first (stop CRD reconciliation of connector/archiver/signal deployments)
// 2. Worker (stop Temporal activities that orchestrate other components)
// 3. Data producers (connectors, lifecycle)
// 4. Data consumers (signals, momentum, notifier)
// 5. Pipeline (cdc, cache — passive consumers, safe to stop late)
// 6. Read-only services last (data-api, agent)
//
// NOT included (infrastructure — always running):
//   ssmd-postgres (StatefulSet), ssmd-redis, ssmd-debug (utility pod)
interface Component {
  label: string;
  deployment?: string;
  selector?: string;
  podLabel?: string; // explicit pod label for waitForPodsTerminated
}

const COMPONENTS: Component[] = [
  { label: "operator", deployment: "ssmd-operator" },
  { label: "worker", deployment: "ssmd-worker", podLabel: "app=ssmd-worker" },
  { label: "connectors", selector: "app.kubernetes.io/name=ssmd-connector" },
  { label: "lifecycle-consumer", deployment: "ssmd-lifecycle-consumer" },
  { label: "funding-rate-consumer", deployment: "ssmd-funding-rate-consumer" },
  { label: "signals", selector: "app.kubernetes.io/name=ssmd-signal" },
  { label: "momentum", deployment: "ssmd-momentum", podLabel: "app=ssmd-momentum" },
  { label: "notifier", deployment: "ssmd-notifier", podLabel: "app=ssmd-notifier" },
  { label: "cdc", deployment: "ssmd-cdc", podLabel: "app=ssmd-cdc" },
  { label: "cache", deployment: "ssmd-cache", podLabel: "app=ssmd-cache" },
  { label: "archiver", selector: "app.kubernetes.io/name=ssmd-archiver" },
  { label: "data-api", deployment: "ssmd-data-ts", podLabel: "app=ssmd-data-ts" },
  { label: "agent", deployment: "ssmd-agent", podLabel: "app=ssmd-agent" },
];


export async function handleScale(
  subcommand: string,
  flags: ScaleFlags
): Promise<void> {
  const opts: KubectlOptions = { env: flags.env };
  const dryRun = flags["dry-run"] ?? false;

  switch (subcommand) {
    case "down":
      await scaleDown(opts, dryRun);
      break;
    case "up":
      await scaleUp(opts, dryRun);
      break;
    case "status":
    case undefined:
      await scaleStatus(opts);
      break;
    default:
      console.error(`Unknown scale command: ${subcommand}`);
      printScaleHelp();
      Deno.exit(1);
  }
}

async function scaleDown(opts: KubectlOptions, dryRun: boolean): Promise<void> {
  const envDisplay = await getCurrentEnvDisplay(opts.env);
  const context = await getEnvContext(opts.env);

  console.log(`Scaling down SSMD components in ${envDisplay}...\n`);

  // Determine kustomization name based on environment
  const kustomizationName = context.envName === "prod" ? "ssmd" : "apps";

  // Suspend Flux kustomization first to prevent reconciliation
  console.log("Suspending Flux kustomization...");
  if (!dryRun) {
    try {
      await flux(["suspend", "kustomization", kustomizationName], opts);
      console.log(`  Flux ${kustomizationName} kustomization suspended\n`);
    } catch (e) {
      console.error(`  Failed to suspend Flux: ${e}`);
      console.error("  Aborting scale down to prevent drift/restore loop");
      Deno.exit(1);
    }
  } else {
    console.log(`[dry-run] Would suspend Flux kustomization ${kustomizationName}\n`);
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
          "-l", component.selector,
          "--replicas=0",
        ], opts);
      } else if (component.deployment) {
        await kubectl([
          "scale", "deployment", component.deployment,
          "--replicas=0",
        ], opts);
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
    await waitForPodsTerminated(opts, 120);
  }

  console.log("\nScale down complete.");
  console.log("Run 'ssmd archiver sync kalshi-archiver' to sync data to GCS.");
}

async function scaleUp(opts: KubectlOptions, dryRun: boolean): Promise<void> {
  const envDisplay = await getCurrentEnvDisplay(opts.env);
  const context = await getEnvContext(opts.env);

  console.log(`Scaling up SSMD components in ${envDisplay} via Flux...\n`);

  // Determine kustomization name based on environment
  const kustomizationName = context.envName === "prod" ? "ssmd" : "apps";

  if (dryRun) {
    console.log(`[dry-run] Would resume Flux kustomization ${kustomizationName}`);
    return;
  }

  // Resume Flux kustomization (this triggers reconciliation automatically)
  console.log("Resuming Flux kustomization...");
  try {
    await flux(["resume", "kustomization", kustomizationName], opts);
    console.log(`  Flux ${kustomizationName} kustomization resumed`);
  } catch (e) {
    console.error(`  Failed to resume Flux: ${e}`);
    Deno.exit(1);
  }

  // Force reconcile to restore immediately
  console.log("Triggering reconciliation...");
  await flux(["reconcile", "kustomization", kustomizationName, "--with-source"], opts);

  console.log("\nScale up triggered. Use 'ssmd scale status' to monitor.");
}

async function scaleStatus(opts: KubectlOptions): Promise<void> {
  const envDisplay = await getCurrentEnvDisplay(opts.env);

  console.log(`SSMD Component Status (${envDisplay})\n`);
  console.log("COMPONENT                          READY   REPLICAS");
  console.log("---------                          -----   --------");

  for (const component of COMPONENTS) {
    try {
      let jsonOutput: string;
      if (component.selector) {
        jsonOutput = await kubectl([
          "get", "deployment",
          "-l", component.selector,
          "-o", "json",
        ], opts);
      } else if (component.deployment) {
        jsonOutput = await kubectl([
          "get", "deployment", component.deployment,
          "-o", "json",
        ], opts);
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

async function waitForPodsTerminated(opts: KubectlOptions, timeoutSec: number): Promise<void> {
  const start = Date.now();
  const selectors = COMPONENTS
    .filter(c => c.selector || c.deployment)
    .map(c => c.selector || c.podLabel || `app.kubernetes.io/name=${c.deployment}`);

  while (Date.now() - start < timeoutSec * 1000) {
    let allTerminated = true;

    for (const selector of selectors) {
      try {
        const output = await kubectl([
          "get", "pods",
          "-l", selector,
          "-o", "jsonpath={.items[*].metadata.name}",
        ], opts);
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

function printScaleHelp(): void {
  console.log("Usage: ssmd [--env <env>] scale <command> [options]");
  console.log("");
  console.log("Commands:");
  console.log("  down      Suspend Flux + scale all SSMD components to 0");
  console.log("  up        Resume Flux + reconcile (restores git-defined replicas)");
  console.log("  status    Show current component status and replica counts");
  console.log("");
  console.log("Options:");
  console.log("  --env <env>  Target environment (default: current from 'ssmd env')");
  console.log("  --dry-run    Show what would be done without making changes");
  console.log("");
  console.log("Examples:");
  console.log("  ssmd scale status");
  console.log("  ssmd --env dev scale status");
  console.log("  ssmd scale down --dry-run");
  console.log("  ssmd scale down");
  console.log("  ssmd archiver sync kalshi-archiver   # sync to GCS after scale down");
  console.log("  ssmd scale up");
}
