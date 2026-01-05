// archiver-deploy.ts - Archiver CR deployment management
// Manages Archiver CRs via kubectl (kubernetes deployment lifecycle)

interface ArchiverDeployFlags {
  _: (string | number)[];
  follow?: boolean;
  tail?: string;
  namespace?: string;
}

const DEFAULT_NAMESPACE = "ssmd";

export async function handleArchiverDeploy(
  subcommand: string,
  flags: ArchiverDeployFlags
): Promise<void> {
  const ns = flags.namespace ?? DEFAULT_NAMESPACE;

  switch (subcommand) {
    case "deploy":
      await deployArchiver(flags, ns);
      break;
    case "list":
      await listArchivers(ns);
      break;
    case "status":
      await statusArchiver(flags, ns);
      break;
    case "logs":
      await logsArchiver(flags, ns);
      break;
    case "delete":
      await deleteArchiver(flags, ns);
      break;
    default:
      console.error(`Unknown archiver-deploy command: ${subcommand}`);
      printArchiverDeployHelp();
      Deno.exit(1);
  }
}

async function deployArchiver(flags: ArchiverDeployFlags, ns: string): Promise<void> {
  const file = flags._[2] as string;

  if (!file) {
    console.error("Usage: ssmd archiver deploy <file.yaml>");
    Deno.exit(1);
  }

  // Check if file exists
  try {
    await Deno.stat(file);
  } catch {
    console.error(`File not found: ${file}`);
    Deno.exit(1);
  }

  console.log(`Deploying Archiver from ${file}...`);

  try {
    const output = await kubectl(["apply", "-f", file, "-n", ns]);
    console.log(output.trim());
  } catch (e) {
    console.error(`Failed to deploy: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function listArchivers(ns: string): Promise<void> {
  console.log("Archiver CRs:\n");
  console.log(
    "NAME".padEnd(30) +
    "DATE".padEnd(12) +
    "STREAM".padEnd(15) +
    "PHASE".padEnd(12) +
    "AGE"
  );
  console.log(
    "----".padEnd(30) +
    "----".padEnd(12) +
    "------".padEnd(15) +
    "-----".padEnd(12) +
    "---"
  );

  try {
    // Get archivers with all needed fields
    const archivers = await kubectl([
      "get", "archiver", "-n", ns,
      "-o", "jsonpath={range .items[*]}{.metadata.name}|{.spec.date}|{.spec.source.stream}|{.status.phase}|{.metadata.creationTimestamp}\\n{end}"
    ]).catch(() => "");

    if (!archivers.trim()) {
      console.log("(no archivers found)");
      return;
    }

    for (const line of archivers.split("\n").filter(Boolean)) {
      const [name, date, stream, phase, createdAt] = line.split("|");

      const age = createdAt ? formatAge(createdAt) : "-";

      console.log(
        (name || "-").padEnd(30) +
        (date || "-").padEnd(12) +
        (stream || "-").padEnd(15) +
        (phase || "-").padEnd(12) +
        age
      );
    }
  } catch (e) {
    console.error(`Failed to list archivers: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function statusArchiver(flags: ArchiverDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd archiver status <name>");
    Deno.exit(1);
  }

  try {
    // Get full Archiver CR as JSON
    const archiverJson = await kubectl([
      "get", "archiver", name, "-n", ns, "-o", "json"
    ]);

    const archiver = JSON.parse(archiverJson);

    console.log(`Archiver: ${archiver.metadata.name}`);
    console.log(`Namespace: ${archiver.metadata.namespace}`);
    console.log(`Created: ${archiver.metadata.creationTimestamp} (${formatAge(archiver.metadata.creationTimestamp)})`);
    console.log();

    // Spec
    console.log("Spec:");
    console.log(`  Date: ${archiver.spec.date || "-"}`);
    console.log(`  Image: ${archiver.spec.image || "(from feed ConfigMap)"}`);
    if (archiver.spec.replicas !== undefined) {
      console.log(`  Replicas: ${archiver.spec.replicas}`);
    }
    console.log();

    // Source
    if (archiver.spec.source) {
      console.log("  Source:");
      console.log(`    Stream: ${archiver.spec.source.stream || "-"}`);
      if (archiver.spec.source.url) {
        console.log(`    URL: ${archiver.spec.source.url}`);
      }
      if (archiver.spec.source.consumer) {
        console.log(`    Consumer: ${archiver.spec.source.consumer}`);
      }
      if (archiver.spec.source.filter) {
        console.log(`    Filter: ${archiver.spec.source.filter}`);
      }
      console.log();
    }

    // Storage - Local
    if (archiver.spec.storage?.local) {
      console.log("  Storage (Local):");
      if (archiver.spec.storage.local.path) {
        console.log(`    Path: ${archiver.spec.storage.local.path}`);
      }
      if (archiver.spec.storage.local.pvcName) {
        console.log(`    PVC Name: ${archiver.spec.storage.local.pvcName}`);
      }
      if (archiver.spec.storage.local.pvcSize) {
        console.log(`    PVC Size: ${archiver.spec.storage.local.pvcSize}`);
      }
      if (archiver.spec.storage.local.storageClass) {
        console.log(`    Storage Class: ${archiver.spec.storage.local.storageClass}`);
      }
      console.log();
    }

    // Storage - Remote
    if (archiver.spec.storage?.remote) {
      console.log("  Storage (Remote):");
      if (archiver.spec.storage.remote.type) {
        console.log(`    Type: ${archiver.spec.storage.remote.type}`);
      }
      if (archiver.spec.storage.remote.bucket) {
        console.log(`    Bucket: ${archiver.spec.storage.remote.bucket}`);
      }
      if (archiver.spec.storage.remote.prefix) {
        console.log(`    Prefix: ${archiver.spec.storage.remote.prefix}`);
      }
      if (archiver.spec.storage.remote.secretRef) {
        console.log(`    Secret Ref: ${archiver.spec.storage.remote.secretRef}`);
      }
      console.log();
    }

    // Rotation
    if (archiver.spec.rotation) {
      console.log("  Rotation:");
      if (archiver.spec.rotation.maxFileAge) {
        console.log(`    Max File Age: ${archiver.spec.rotation.maxFileAge}`);
      }
      if (archiver.spec.rotation.maxFileSize) {
        console.log(`    Max File Size: ${archiver.spec.rotation.maxFileSize}`);
      }
      console.log();
    }

    // Sync
    if (archiver.spec.sync) {
      console.log("  Sync:");
      console.log(`    Enabled: ${archiver.spec.sync.enabled ?? true}`);
      if (archiver.spec.sync.schedule) {
        console.log(`    Schedule: ${archiver.spec.sync.schedule}`);
      }
      if (archiver.spec.sync.onDelete) {
        console.log(`    On Delete: ${archiver.spec.sync.onDelete}`);
      }
      console.log();
    }

    // Status
    console.log("Status:");
    console.log(`  Phase: ${archiver.status?.phase || "Unknown"}`);
    if (archiver.status?.deployment) {
      console.log(`  Deployment: ${archiver.status.deployment}`);
    }
    console.log(`  Messages Archived: ${(archiver.status?.messagesArchived || 0).toLocaleString()}`);
    console.log(`  Bytes Written: ${formatBytes(archiver.status?.bytesWritten || 0)}`);
    console.log(`  Files Written: ${archiver.status?.filesWritten || 0}`);
    if (archiver.status?.lastFlushAt) {
      console.log(`  Last Flush At: ${archiver.status.lastFlushAt} (${formatAge(archiver.status.lastFlushAt)})`);
    }
    if (archiver.status?.lastSyncAt) {
      console.log(`  Last Sync At: ${archiver.status.lastSyncAt} (${formatAge(archiver.status.lastSyncAt)})`);
    }
    if (archiver.status?.lastSyncFiles !== undefined) {
      console.log(`  Last Sync Files: ${archiver.status.lastSyncFiles}`);
    }
    if (archiver.status?.pendingSyncBytes !== undefined) {
      console.log(`  Pending Sync Bytes: ${formatBytes(archiver.status.pendingSyncBytes)}`);
    }
    console.log();

    // Conditions
    if (archiver.status?.conditions?.length) {
      console.log("Conditions:");
      for (const cond of archiver.status.conditions) {
        const status = cond.status === "True" ? "+" : "-";
        console.log(`  [${status}] ${cond.type}: ${cond.reason}`);
        if (cond.message) {
          console.log(`      ${cond.message}`);
        }
      }
    }
  } catch (e) {
    console.error(`Failed to get archiver status: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function logsArchiver(flags: ArchiverDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd archiver logs <name> [--follow] [--tail N]");
    Deno.exit(1);
  }

  try {
    // First get the deployment name from the Archiver CR
    const deploymentName = await kubectl([
      "get", "archiver", name, "-n", ns,
      "-o", "jsonpath={.status.deployment}"
    ]).catch(() => "");

    // If no deployment in status, try the conventional name
    const targetDeployment = deploymentName.trim() || `archiver-${name}`;

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

async function deleteArchiver(flags: ArchiverDeployFlags, ns: string): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd archiver delete <name>");
    Deno.exit(1);
  }

  console.log(`Deleting Archiver ${name}...`);

  try {
    await kubectl(["delete", "archiver", name, "-n", ns]);
    console.log(`Archiver ${name} deleted`);
  } catch (e) {
    console.error(`Failed to delete archiver: ${e instanceof Error ? e.message : e}`);
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

export function formatAge(timestamp: string): string {
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

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";

  const units = ["B", "KB", "MB", "GB", "TB"];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${units[i]}`;
}

export function printArchiverDeployHelp(): void {
  console.log("Usage: ssmd archiver <deploy-command> [options]");
  console.log();
  console.log("Kubernetes Archiver CR Management Commands:");
  console.log("  deploy <file.yaml>     Deploy an Archiver CR from YAML file");
  console.log("  list                   List all Archiver CRs");
  console.log("  status <name>          Show detailed Archiver status");
  console.log("  logs <name>            Show logs from Archiver pod");
  console.log("  delete <name>          Delete an Archiver CR");
  console.log();
  console.log("Options:");
  console.log("  --namespace NS         Kubernetes namespace (default: ssmd)");
  console.log("  --follow, -f           Follow log output (logs command)");
  console.log("  --tail N               Number of lines to show (logs command)");
  console.log();
  console.log("Examples:");
  console.log("  ssmd archiver deploy archivers/kalshi-2026-01-04.yaml");
  console.log("  ssmd archiver list");
  console.log("  ssmd archiver status kalshi-2026-01-04");
  console.log("  ssmd archiver logs kalshi-2026-01-04 --follow --tail 100");
  console.log("  ssmd archiver delete kalshi-2026-01-04");
}
