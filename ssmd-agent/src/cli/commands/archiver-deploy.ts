// archiver-deploy.ts - Archiver CR deployment management
// Manages Archiver CRs via kubectl (kubernetes deployment lifecycle)

import { kubectl, kubectlStream, getCurrentEnvDisplay, type KubectlOptions } from "../utils/kubectl.ts";
import { getEnvContext } from "../utils/env-context.ts";
import { getRawSql, closeDb } from "../../lib/db/mod.ts";

interface ArchiverDeployFlags {
  _: (string | number)[];
  follow?: boolean;
  tail?: string;
  namespace?: string;
  env?: string;
  wait?: boolean;
  // Flags for 'new' command
  date?: string;
  stream?: string;
  filter?: string;
  "local-path"?: string;
  "pvc-name"?: string;
  "gcs-bucket"?: string;
  "gcs-prefix"?: string;
  image?: string;
  output?: string;
}

export async function handleArchiverDeploy(
  subcommand: string,
  flags: ArchiverDeployFlags
): Promise<void> {
  const opts: KubectlOptions = {
    env: flags.env,
    namespace: flags.namespace,
  };

  switch (subcommand) {
    case "new":
      await newArchiver(flags, opts);
      break;
    case "deploy":
      await deployArchiver(flags, opts);
      break;
    case "list":
      await listArchivers(opts);
      break;
    case "status":
      await statusArchiver(flags, opts);
      break;
    case "logs":
      await logsArchiver(flags, opts);
      break;
    case "delete":
      await deleteArchiver(flags, opts);
      break;
    case "sync":
      await syncArchiver(flags, opts);
      break;
    default:
      console.error(`Unknown archiver-deploy command: ${subcommand}`);
      printArchiverDeployHelp();
      Deno.exit(1);
  }
}

const DEFAULT_IMAGE = "ghcr.io/aaronwald/ssmd-archiver:0.4.8";

async function newArchiver(flags: ArchiverDeployFlags, opts: KubectlOptions): Promise<void> {
  const name = flags._[2] as string;
  const context = await getEnvContext(opts.env);
  const ns = opts.namespace ?? context.namespace;

  if (!name) {
    console.error("Usage: ssmd archiver new <name> --stream <stream> --filter <filter> [options]");
    console.error("\nRequired flags:");
    console.error("  --stream          NATS stream name (e.g., PROD_KALSHI)");
    console.error("  --filter          NATS subject filter (e.g., prod.kalshi.main.json.>)");
    console.error("\nOptional flags:");
    console.error("  --date            Date (default: today, YYYY-MM-DD)");
    console.error("  --local-path      Local storage path (default: /data/ssmd/<feed>/<date>)");
    console.error("  --pvc-name        PVC name (default: ssmd-archiver-data)");
    console.error("  --gcs-bucket      GCS bucket for remote sync");
    console.error("  --gcs-prefix      GCS prefix (default: derived from date)");
    console.error("  --image           Container image (default: " + DEFAULT_IMAGE + ")");
    console.error("  --output          Output file (default: stdout)");
    Deno.exit(1);
  }

  const stream = flags.stream;
  const filter = flags.filter;

  if (!stream || !filter) {
    console.error("Error: --stream and --filter are required");
    console.error("\nExample:");
    console.error("  ssmd archiver new kalshi-2026-01-05 \\");
    console.error("    --stream PROD_KALSHI \\");
    console.error("    --filter prod.kalshi.main.json.>");
    Deno.exit(1);
  }

  // Default date to today
  const date = flags.date ?? new Date().toISOString().split("T")[0];
  const [year, month, day] = date.split("-");
  const image = flags.image ?? DEFAULT_IMAGE;
  const localPath = flags["local-path"] ?? `/data/ssmd/kalshi/${year}/${month}/${day}`;
  const pvcName = flags["pvc-name"] ?? "ssmd-archiver-data";
  const gcsBucket = flags["gcs-bucket"] ?? "ssmd-archive";
  const gcsPrefix = flags["gcs-prefix"] ?? `kalshi/${year}/${month}/${day}`;

  const yaml = `apiVersion: ssmd.ssmd.io/v1alpha1
kind: Archiver
metadata:
  name: ${name}
  namespace: ${ns}
spec:
  date: "${date}"
  image: ${image}
  source:
    stream: ${stream}
    url: nats://nats.nats.svc.cluster.local:4222
    consumer: archiver-${date}
    filter: "${filter}"
  storage:
    local:
      path: ${localPath}
      pvcName: ${pvcName}
    remote:
      type: gcs
      bucket: ${gcsBucket}
      prefix: ${gcsPrefix}
      secretRef: gcs-credentials
  rotation:
    maxFileAge: "15m"
  sync:
    enabled: true
  resources:
    requests:
      cpu: 100m
      memory: 128Mi
    limits:
      cpu: 500m
      memory: 512Mi
`;

  if (flags.output) {
    await Deno.writeTextFile(flags.output, yaml);
    console.log(`Wrote archiver YAML to ${flags.output}`);
    console.log("\nNext steps:");
    console.log(`  1. Review ${flags.output} and adjust settings as needed`);
    console.log(`  2. ssmd archiver deploy ${flags.output}`);
  } else {
    console.log(yaml);
  }
}

async function deployArchiver(flags: ArchiverDeployFlags, opts: KubectlOptions): Promise<void> {
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

  const envDisplay = await getCurrentEnvDisplay(opts.env);
  console.log(`Deploying Archiver from ${file} to ${envDisplay}...`);

  try {
    const output = await kubectl(["apply", "-f", file], opts);
    console.log(output.trim());
  } catch (e) {
    console.error(`Failed to deploy: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function listArchivers(opts: KubectlOptions): Promise<void> {
  const envDisplay = await getCurrentEnvDisplay(opts.env);
  console.log(`Archiver CRs (${envDisplay}):\n`);
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
      "get", "archiver",
      "-o", "jsonpath={range .items[*]}{.metadata.name}|{.spec.date}|{.spec.source.stream}|{.status.phase}|{.metadata.creationTimestamp}\\n{end}"
    ], opts).catch(() => "");

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

async function statusArchiver(flags: ArchiverDeployFlags, opts: KubectlOptions): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd archiver status <name>");
    Deno.exit(1);
  }

  try {
    // Get full Archiver CR as JSON
    const archiverJson = await kubectl([
      "get", "archiver", name, "-o", "json"
    ], opts);

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

async function logsArchiver(flags: ArchiverDeployFlags, opts: KubectlOptions): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd archiver logs <name> [--follow] [--tail N]");
    Deno.exit(1);
  }

  try {
    // First get the deployment name from the Archiver CR
    const deploymentName = await kubectl([
      "get", "archiver", name,
      "-o", "jsonpath={.status.deployment}"
    ], opts).catch(() => "");

    // If no deployment in status, try the conventional name
    const targetDeployment = deploymentName.trim() || `${name}-archiver`;

    // Build kubectl logs args
    const logsArgs = ["logs", `deployment/${targetDeployment}`];

    if (flags.follow) {
      logsArgs.push("-f");
    }

    if (flags.tail !== undefined) {
      logsArgs.push("--tail", String(flags.tail));
    }

    // Stream logs directly to stdout/stderr
    await kubectlStream(logsArgs, opts);
  } catch (e) {
    console.error(`Failed to get logs: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function deleteArchiver(flags: ArchiverDeployFlags, opts: KubectlOptions): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd archiver delete <name>");
    Deno.exit(1);
  }

  const envDisplay = await getCurrentEnvDisplay(opts.env);
  console.log(`Deleting Archiver ${name} from ${envDisplay}...`);

  try {
    await kubectl(["delete", "archiver", name], opts);
    console.log(`Archiver ${name} deleted`);
  } catch (e) {
    console.error(`Failed to delete archiver: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function syncArchiver(flags: ArchiverDeployFlags, opts: KubectlOptions): Promise<void> {
  const name = flags._[2] as string;
  const context = await getEnvContext(opts.env);
  const ns = opts.namespace ?? context.namespace;

  if (!name) {
    console.error("Usage: ssmd archiver sync <name> [--wait]");
    Deno.exit(1);
  }

  const envDisplay = await getCurrentEnvDisplay(opts.env);
  console.log(`Creating GCS sync job for archiver ${name} in ${envDisplay}...`);

  try {
    // Get the archiver CR to read sync config
    const archiverJson = await kubectl([
      "get", "archiver", name, "-o", "json"
    ], opts);
    const archiver = JSON.parse(archiverJson);

    // Validate sync is configured
    const remote = archiver.spec.storage?.remote;
    const local = archiver.spec.storage?.local;

    if (!remote?.bucket) {
      console.error("Error: Archiver has no remote storage configured");
      Deno.exit(1);
    }

    if (!local?.pvcName) {
      console.error("Error: Archiver has no local PVC configured");
      Deno.exit(1);
    }

    // Build paths
    const localPath = local.path || "/data/ssmd";
    const feed = archiver.spec.feed || "kalshi";
    const remotePath = remote.prefix
      ? `gs://${remote.bucket}/${remote.prefix}/`
      : `gs://${remote.bucket}/`;
    const secretRef = remote.secretRef || "gcs-credentials";

    // Delete existing sync job if any
    const jobName = `${name}-sync`;
    await kubectl(["delete", "job", jobName, "--ignore-not-found"], opts).catch(() => {});

    // Create sync job YAML
    const jobYaml = `apiVersion: batch/v1
kind: Job
metadata:
  name: ${jobName}
  namespace: ${ns}
  labels:
    app.kubernetes.io/name: ssmd-archiver-sync
    app.kubernetes.io/instance: ${name}
spec:
  ttlSecondsAfterFinished: 3600
  template:
    metadata:
      labels:
        app.kubernetes.io/name: ssmd-archiver-sync
        app.kubernetes.io/instance: ${name}
    spec:
      restartPolicy: Never
      containers:
        - name: sync
          image: gcr.io/google.com/cloudsdktool/google-cloud-cli:slim
          command:
            - sh
            - -c
            - |
              set -e
              gcloud auth activate-service-account --key-file=/etc/gcs/key.json
              LOCAL="${localPath}/${feed}"
              REMOTE="${remotePath}"
              if [ -d "$LOCAL" ] && [ "$(ls -A $LOCAL 2>/dev/null)" ]; then
                echo "Syncing $LOCAL to $REMOTE"
                gsutil -m rsync -r "$LOCAL" "$REMOTE"
                echo "Sync complete"
              else
                echo "No data at $LOCAL, nothing to sync"
              fi
          volumeMounts:
            - name: data
              mountPath: /data
            - name: gcs-credentials
              mountPath: /etc/gcs
              readOnly: true
          env:
            - name: GOOGLE_APPLICATION_CREDENTIALS
              value: /etc/gcs/key.json
      volumes:
        - name: data
          persistentVolumeClaim:
            claimName: ${local.pvcName}
        - name: gcs-credentials
          secret:
            secretName: ${secretRef}
`;

    // Apply the job using kubectl with context
    const applyCmd = new Deno.Command("kubectl", {
      args: ["--context", context.cluster, "-n", ns, "apply", "-f", "-"],
      stdin: "piped",
      stdout: "piped",
      stderr: "piped",
    });

    const child = applyCmd.spawn();
    const writer = child.stdin.getWriter();
    await writer.write(new TextEncoder().encode(jobYaml));
    await writer.close();

    const { code, stderr } = await child.output();
    if (code !== 0) {
      throw new Error(new TextDecoder().decode(stderr));
    }

    console.log(`Sync job ${jobName} created`);
    console.log(`  Local: ${localPath}/${feed}`);
    console.log(`  Remote: ${remotePath}`);

    // Wait for job if requested
    if (flags.wait) {
      console.log("\nWaiting for sync job to complete...");
      const syncStartTime = Date.now();
      await waitForJob(jobName, opts, 600); // 10 minute timeout

      // Log successful sync to DB
      try {
        const db = getRawSql();
        await db`
          INSERT INTO archiver_sync_log (archiver_name, success, duration_ms)
          VALUES (${name}, true, ${Date.now() - syncStartTime})
        `;
        await closeDb();
      } catch (e) {
        console.error(`WARN: Failed to log sync: ${e}`);
      }
    } else {
      console.log(`\nMonitor with: kubectl logs -n ${ns} job/${jobName} -f`);
    }
  } catch (e) {
    console.error(`Failed to create sync job: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function waitForJob(name: string, opts: KubectlOptions, timeoutSec: number): Promise<void> {
  const start = Date.now();

  while (Date.now() - start < timeoutSec * 1000) {
    try {
      const status = await kubectl([
        "get", "job", name,
        "-o", "jsonpath={.status.succeeded},{.status.failed}"
      ], opts);

      const [succeeded, failed] = status.split(",").map(s => parseInt(s) || 0);

      if (succeeded > 0) {
        console.log("Sync job completed successfully");
        return;
      }

      if (failed > 0) {
        console.error("Sync job failed");
        const context = await getEnvContext(opts.env);
        console.log(`Check logs: kubectl logs -n ${context.namespace} job/${name}`);
        Deno.exit(1);
      }
    } catch {
      // Job may not exist yet
    }

    await new Promise(r => setTimeout(r, 5000));
  }

  console.error(`Timeout waiting for sync job (${timeoutSec}s)`);
  Deno.exit(1);
}

// Helper functions

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
  console.log("Usage: ssmd [--env <env>] archiver <command> [options]");
  console.log();
  console.log("Kubernetes Archiver CR Management Commands:");
  console.log("  new <name>             Generate a new Archiver CR YAML");
  console.log("  deploy <file.yaml>     Deploy an Archiver CR from YAML file");
  console.log("  list                   List all Archiver CRs");
  console.log("  status <name>          Show detailed Archiver status");
  console.log("  logs <name>            Show logs from Archiver pod");
  console.log("  delete <name>          Delete an Archiver CR");
  console.log("  sync <name>            Trigger GCS sync job for archiver data");
  console.log();
  console.log("Options for 'new':");
  console.log("  --stream <stream>      NATS stream name (required, e.g., PROD_KALSHI)");
  console.log("  --filter <filter>      NATS subject filter (required, e.g., prod.kalshi.main.json.>)");
  console.log("  --date <YYYY-MM-DD>    Date (default: today)");
  console.log("  --local-path <path>    Local storage path");
  console.log("  --pvc-name <name>      PVC name (default: ssmd-archiver-data)");
  console.log("  --gcs-bucket <bucket>  GCS bucket (default: ssmd-archive)");
  console.log("  --gcs-prefix <prefix>  GCS prefix");
  console.log("  --image <image>        Container image (default: latest)");
  console.log("  --output <file>        Output file (default: stdout)");
  console.log();
  console.log("Options for other commands:");
  console.log("  --env <env>            Target environment (default: current from 'ssmd env')");
  console.log("  --namespace NS         Override namespace (default: from environment)");
  console.log("  --follow, -f           Follow log output (logs command)");
  console.log("  --tail N               Number of lines to show (logs command)");
  console.log();
  console.log("Options for 'sync':");
  console.log("  --wait                 Wait for sync job to complete");
  console.log();
  console.log("Examples:");
  console.log("  ssmd archiver new kalshi-2026-01-05 \\");
  console.log("    --stream PROD_KALSHI --filter prod.kalshi.main.json.> \\");
  console.log("    --output archiver.yaml");
  console.log("  ssmd archiver deploy archiver.yaml");
  console.log("  ssmd --env dev archiver list");
  console.log("  ssmd archiver status kalshi-2026-01-05");
  console.log("  ssmd archiver logs kalshi-2026-01-05 --follow --tail 100");
  console.log("  ssmd archiver sync kalshi-archiver");
  console.log("  ssmd archiver sync kalshi-archiver --wait");
  console.log("  ssmd archiver delete kalshi-2026-01-05");
}
