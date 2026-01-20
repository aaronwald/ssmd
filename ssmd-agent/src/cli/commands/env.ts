// env.ts - Environment management commands
// ssmd env list|use|current|show

import {
  loadConfig,
  getCurrentEnvName,
  setCurrentEnv,
  getEnv,
  listEnvNames,
  getConfigPath,
} from "../utils/env-context.ts";

interface EnvFlags {
  _: (string | number)[];
}

export async function handleEnv(
  subcommand: string,
  flags: EnvFlags
): Promise<void> {
  switch (subcommand) {
    case "list":
    case undefined:
      await envList();
      break;
    case "use":
      await envUse(flags._[2] as string);
      break;
    case "current":
      await envCurrent();
      break;
    case "show":
      await envShow(flags._[2] as string);
      break;
    default:
      console.error(`Unknown env command: ${subcommand}`);
      printEnvHelp();
      Deno.exit(1);
  }
}

async function envList(): Promise<void> {
  const config = await loadConfig();
  const currentEnv = config["current-env"];
  const envNames = Object.keys(config.environments);

  console.log("Environments:\n");
  console.log(
    "NAME".padEnd(15) +
    "CLUSTER".padEnd(20) +
    "NAMESPACE".padEnd(15) +
    "CURRENT"
  );
  console.log(
    "----".padEnd(15) +
    "-------".padEnd(20) +
    "---------".padEnd(15) +
    "-------"
  );

  for (const name of envNames) {
    const env = config.environments[name];
    const isCurrent = name === currentEnv ? "*" : "";
    console.log(
      name.padEnd(15) +
      env.cluster.padEnd(20) +
      env.namespace.padEnd(15) +
      isCurrent
    );
  }

  console.log(`\nConfig: ${getConfigPath()}`);
}

async function envUse(name: string): Promise<void> {
  if (!name) {
    console.error("Usage: ssmd env use <name>");
    console.log("\nAvailable environments:");
    const names = await listEnvNames();
    for (const n of names) {
      console.log(`  ${n}`);
    }
    Deno.exit(1);
  }

  try {
    await setCurrentEnv(name);
    const env = await getEnv(name);
    console.log(`Switched to environment: ${name}`);
    console.log(`  Cluster: ${env.cluster}`);
    console.log(`  Namespace: ${env.namespace}`);
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    Deno.exit(1);
  }
}

async function envCurrent(): Promise<void> {
  const name = await getCurrentEnvName();
  const env = await getEnv(name);

  console.log(name);
  console.log(`  Cluster: ${env.cluster}`);
  console.log(`  Namespace: ${env.namespace}`);
}

async function envShow(name?: string): Promise<void> {
  if (!name) {
    name = await getCurrentEnvName();
  }

  try {
    const env = await getEnv(name);

    console.log(`Environment: ${name}\n`);
    console.log(`Cluster: ${env.cluster}`);
    console.log(`Namespace: ${env.namespace}`);
    console.log();

    console.log("NATS:");
    console.log(`  URL: ${env.nats.url}`);
    console.log(`  Stream Prefix: ${env.nats.stream_prefix}`);

    if (env.storage) {
      console.log();
      console.log("Storage:");
      console.log(`  Type: ${env.storage.type}`);
      if (env.storage.type === "s3") {
        console.log(`  Bucket: ${env.storage.bucket}`);
        if (env.storage.region) {
          console.log(`  Region: ${env.storage.region}`);
        }
      } else if (env.storage.type === "gcs") {
        console.log(`  Bucket: ${env.storage.bucket}`);
      } else if (env.storage.type === "local") {
        console.log(`  Path: ${env.storage.path}`);
      }
    }

    if (env.secrets?.kalshi) {
      console.log();
      console.log("Secrets:");
      console.log(`  Kalshi: ${env.secrets.kalshi}`);
    }
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    Deno.exit(1);
  }
}

function printEnvHelp(): void {
  console.log("Usage: ssmd env <command> [options]");
  console.log();
  console.log("Commands:");
  console.log("  list              List all configured environments");
  console.log("  use <name>        Switch to a different environment");
  console.log("  current           Show current environment");
  console.log("  show [name]       Show environment details (defaults to current)");
  console.log();
  console.log("Examples:");
  console.log("  ssmd env list");
  console.log("  ssmd env use dev");
  console.log("  ssmd env current");
  console.log("  ssmd env show prod");
  console.log();
  console.log(`Config file: ${getConfigPath()}`);
}
