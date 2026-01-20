// connector-deploy.ts - Connector CR deployment management
// Manages Connector CRs via kubectl (kubernetes deployment lifecycle)

import { kubectl, kubectlStream, getCurrentEnvDisplay, type KubectlOptions } from "../utils/kubectl.ts";

interface ConnectorDeployFlags {
  _: (string | number)[];
  follow?: boolean;
  tail?: string;
  namespace?: string;
  env?: string;
  // Flags for 'new' command
  feed?: string;
  stream?: string;
  "subject-prefix"?: string;
  image?: string;
  output?: string;
}

export async function handleConnectorDeploy(
  subcommand: string,
  flags: ConnectorDeployFlags
): Promise<void> {
  // Build kubectl options from flags
  const opts: KubectlOptions = {
    env: flags.env,
    namespace: flags.namespace,
  };

  switch (subcommand) {
    case "new":
      await newConnector(flags, opts);
      break;
    case "deploy":
      await deployConnector(flags, opts);
      break;
    case "list":
      await listConnectors(opts);
      break;
    case "status":
      await statusConnector(flags, opts);
      break;
    case "logs":
      await logsConnector(flags, opts);
      break;
    case "delete":
      await deleteConnector(flags, opts);
      break;
    default:
      console.error(`Unknown connector-deploy command: ${subcommand}`);
      printConnectorDeployHelp();
      Deno.exit(1);
  }
}

const DEFAULT_IMAGE = "ghcr.io/aaronwald/ssmd-connector:0.5.5";

async function newConnector(flags: ConnectorDeployFlags, opts: KubectlOptions): Promise<void> {
  const name = flags._[2] as string;
  const { envName, namespace } = await import("../utils/env-context.ts").then(m => m.getEnvContext(opts.env));
  const ns = opts.namespace ?? namespace;

  if (!name) {
    console.error("Usage: ssmd connector new <name> --feed <feed> --stream <stream> --subject-prefix <prefix> [options]");
    console.error("\nRequired flags:");
    console.error("  --feed            Feed name (e.g., kalshi)");
    console.error("  --stream          NATS stream name (e.g., PROD_KALSHI)");
    console.error("  --subject-prefix  Subject prefix (e.g., prod.kalshi.main)");
    console.error("\nOptional flags:");
    console.error("  --image           Container image (default: " + DEFAULT_IMAGE + ")");
    console.error("  --output          Output file (default: stdout)");
    Deno.exit(1);
  }

  const feed = flags.feed;
  const stream = flags.stream;
  const subjectPrefix = flags["subject-prefix"];

  if (!feed || !stream || !subjectPrefix) {
    console.error("Error: --feed, --stream, and --subject-prefix are required");
    console.error("\nExample:");
    console.error("  ssmd connector new kalshi-economics \\");
    console.error("    --feed kalshi \\");
    console.error("    --stream PROD_KALSHI_ECONOMICS \\");
    console.error("    --subject-prefix prod.kalshi.economics");
    Deno.exit(1);
  }

  const image = flags.image ?? DEFAULT_IMAGE;

  const yaml = `apiVersion: ssmd.ssmd.io/v1alpha1
kind: Connector
metadata:
  name: ${name}
  namespace: ${ns}
spec:
  feed: ${feed}
  image: ${image}
  # Add feed-specific fields here (e.g., categories for Kalshi)
  # categories:
  #   - Politics
  transport:
    type: nats
    url: nats://nats.nats.svc.cluster.local:4222
    stream: ${stream}
    subjectPrefix: ${subjectPrefix}
  secretRef:
    name: ssmd-${feed}-credentials
    apiKeyField: api-key
    privateKeyField: private-key
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
    console.log(`Wrote connector YAML to ${flags.output}`);
    console.log("\nNext steps:");
    console.log(`  1. Edit ${flags.output} to add feed-specific fields (e.g., categories)`);
    console.log(`  2. ssmd connector deploy ${flags.output}`);
  } else {
    console.log(yaml);
  }
}

async function deployConnector(flags: ConnectorDeployFlags, opts: KubectlOptions): Promise<void> {
  const file = flags._[2] as string;

  if (!file) {
    console.error("Usage: ssmd connector deploy <file.yaml>");
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
  console.log(`Deploying Connector from ${file} to ${envDisplay}...`);

  try {
    const output = await kubectl(["apply", "-f", file], opts);
    console.log(output.trim());
  } catch (e) {
    console.error(`Failed to deploy: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function listConnectors(opts: KubectlOptions): Promise<void> {
  const envDisplay = await getCurrentEnvDisplay(opts.env);
  console.log(`Connector CRs (${envDisplay}):\n`);
  console.log(
    "NAME".padEnd(30) +
    "FEED".padEnd(10) +
    "PHASE".padEnd(12) +
    "AGE"
  );
  console.log(
    "----".padEnd(30) +
    "----".padEnd(10) +
    "-----".padEnd(12) +
    "---"
  );

  try {
    // Get connectors as JSON and parse
    const output = await kubectl([
      "get", "connector", "-o", "json"
    ], opts).catch(() => '{"items":[]}');

    const data = JSON.parse(output);
    const items = data.items || [];

    if (items.length === 0) {
      console.log("(no connectors found)");
      return;
    }

    for (const connector of items) {
      const name = connector.metadata?.name || "-";
      const feed = connector.spec?.feed || "-";
      const phase = connector.status?.phase || "-";
      const createdAt = connector.metadata?.creationTimestamp;
      const age = createdAt ? formatAge(createdAt) : "-";

      console.log(
        name.padEnd(30) +
        feed.padEnd(10) +
        phase.padEnd(12) +
        age
      );
    }
  } catch (e) {
    console.error(`Failed to list connectors: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function statusConnector(flags: ConnectorDeployFlags, opts: KubectlOptions): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd connector status <name>");
    Deno.exit(1);
  }

  try {
    // Get full Connector CR as JSON
    const connectorJson = await kubectl([
      "get", "connector", name, "-o", "json"
    ], opts);

    const connector = JSON.parse(connectorJson);

    console.log(`Connector: ${connector.metadata.name}`);
    console.log(`Namespace: ${connector.metadata.namespace}`);
    console.log(`Created: ${connector.metadata.creationTimestamp} (${formatAge(connector.metadata.creationTimestamp)})`);
    console.log();

    // Spec
    console.log("Spec:");
    console.log(`  Feed: ${connector.spec.feed || "-"}`);
    console.log(`  Image: ${connector.spec.image || "(from feed ConfigMap)"}`);
    if (connector.spec.replicas !== undefined) {
      console.log(`  Replicas: ${connector.spec.replicas}`);
    }
    if (connector.spec.categories?.length) {
      console.log(`  Categories: ${connector.spec.categories.join(", ")}`);
    }
    if (connector.spec.excludeCategories?.length) {
      console.log(`  Exclude Categories: ${connector.spec.excludeCategories.join(", ")}`);
    }
    console.log();

    // Transport
    if (connector.spec.transport) {
      console.log("  Transport:");
      console.log(`    Type: ${connector.spec.transport.type || "nats"}`);
      if (connector.spec.transport.url) {
        console.log(`    URL: ${connector.spec.transport.url}`);
      }
      if (connector.spec.transport.stream) {
        console.log(`    Stream: ${connector.spec.transport.stream}`);
      }
      if (connector.spec.transport.subjectPrefix) {
        console.log(`    Subject Prefix: ${connector.spec.transport.subjectPrefix}`);
      }
      console.log();
    }

    // Secret Reference
    if (connector.spec.secretRef) {
      console.log("  Secret Reference:");
      console.log(`    Name: ${connector.spec.secretRef.name}`);
      if (connector.spec.secretRef.apiKeyField) {
        console.log(`    API Key Field: ${connector.spec.secretRef.apiKeyField}`);
      }
      if (connector.spec.secretRef.privateKeyField) {
        console.log(`    Private Key Field: ${connector.spec.secretRef.privateKeyField}`);
      }
      console.log();
    }

    // Status
    console.log("Status:");
    console.log(`  Phase: ${connector.status?.phase || "Unknown"}`);
    if (connector.status?.deployment) {
      console.log(`  Deployment: ${connector.status.deployment}`);
    }
    if (connector.status?.startedAt) {
      console.log(`  Started At: ${connector.status.startedAt} (${formatAge(connector.status.startedAt)})`);
    }
    console.log(`  Messages Published: ${(connector.status?.messagesPublished || 0).toLocaleString()}`);
    if (connector.status?.lastMessageAt) {
      console.log(`  Last Message At: ${connector.status.lastMessageAt} (${formatAge(connector.status.lastMessageAt)})`);
    }
    if (connector.status?.connectionState) {
      console.log(`  Connection State: ${connector.status.connectionState}`);
    }
    console.log();

    // Conditions
    if (connector.status?.conditions?.length) {
      console.log("Conditions:");
      for (const cond of connector.status.conditions) {
        const status = cond.status === "True" ? "+" : "-";
        console.log(`  [${status}] ${cond.type}: ${cond.reason}`);
        if (cond.message) {
          console.log(`      ${cond.message}`);
        }
      }
    }
  } catch (e) {
    console.error(`Failed to get connector status: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
}

async function logsConnector(flags: ConnectorDeployFlags, opts: KubectlOptions): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd connector logs <name> [--follow] [--tail N]");
    Deno.exit(1);
  }

  try {
    // First get the deployment name from the Connector CR
    const deploymentName = await kubectl([
      "get", "connector", name,
      "-o", "jsonpath={.status.deployment}"
    ], opts).catch(() => "");

    // If no deployment in status, try the conventional name
    const targetDeployment = deploymentName.trim() || `connector-${name}`;

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

async function deleteConnector(flags: ConnectorDeployFlags, opts: KubectlOptions): Promise<void> {
  const name = flags._[2] as string;

  if (!name) {
    console.error("Usage: ssmd connector delete <name>");
    Deno.exit(1);
  }

  const envDisplay = await getCurrentEnvDisplay(opts.env);
  console.log(`Deleting Connector ${name} from ${envDisplay}...`);

  try {
    await kubectl(["delete", "connector", name], opts);
    console.log(`Connector ${name} deleted`);
  } catch (e) {
    console.error(`Failed to delete connector: ${e instanceof Error ? e.message : e}`);
    Deno.exit(1);
  }
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

export function printConnectorDeployHelp(): void {
  console.log("Usage: ssmd [--env <env>] connector <command> [options]");
  console.log();
  console.log("Kubernetes Connector CR Management Commands:");
  console.log("  new <name>             Generate a new Connector CR YAML");
  console.log("  deploy <file.yaml>     Deploy a Connector CR from YAML file");
  console.log("  list                   List all Connector CRs");
  console.log("  status <name>          Show detailed Connector status");
  console.log("  logs <name>            Show logs from Connector pod");
  console.log("  delete <name>          Delete a Connector CR");
  console.log();
  console.log("Options for 'new':");
  console.log("  --feed <feed>          Feed name (required, e.g., kalshi)");
  console.log("  --stream <stream>      NATS stream name (required, e.g., PROD_KALSHI)");
  console.log("  --subject-prefix <p>   Subject prefix (required, e.g., prod.kalshi.main)");
  console.log("  --image <image>        Container image (default: latest)");
  console.log("  --output <file>        Output file (default: stdout)");
  console.log();
  console.log("Options for other commands:");
  console.log("  --env <env>            Target environment (default: current from 'ssmd env')");
  console.log("  --namespace NS         Override namespace (default: from environment)");
  console.log("  --follow, -f           Follow log output (logs command)");
  console.log("  --tail N               Number of lines to show (logs command)");
  console.log();
  console.log("Examples:");
  console.log("  ssmd connector new kalshi-economics \\");
  console.log("    --feed kalshi --stream PROD_KALSHI_ECONOMICS \\");
  console.log("    --subject-prefix prod.kalshi.economics --output connector.yaml");
  console.log("  ssmd connector deploy connector.yaml");
  console.log("  ssmd connector list");
  console.log("  ssmd connector status kalshi-main");
  console.log("  ssmd connector logs kalshi-main --follow --tail 100");
  console.log("  ssmd connector delete kalshi-main");
}
