// day.ts - Trading day lifecycle management
// Manages Connector and Archiver CRs via kubectl

interface DayFlags {
  _: (string | number)[];
  date?: string;
  feed?: string;
  "connector-image"?: string;
  "archiver-image"?: string;
  wait?: boolean;
}

export async function handleDay(
  subcommand: string,
  flags: DayFlags
): Promise<void> {
  switch (subcommand) {
    case "start":
      await startDay(flags);
      break;
    case "end":
      await endDay(flags);
      break;
    case "status":
      await statusDay(flags);
      break;
    case "list":
    case undefined:
      await listDays(flags);
      break;
    default:
      console.error(`Unknown day command: ${subcommand}`);
      printDayHelp();
      Deno.exit(1);
  }
}

async function startDay(flags: DayFlags): Promise<void> {
  const env = flags._[2] as string; // e.g., "kalshi-prod"
  const date = flags.date || new Date().toISOString().split("T")[0];

  if (!env) {
    console.error("Usage: ssmd day start <env> [--date YYYY-MM-DD]");
    Deno.exit(1);
  }

  const feed = env.split("-")[0]; // kalshi-prod -> kalshi

  console.log(`Starting trading day ${date} for ${env}...`);

  // Create Connector CR
  const connectorYaml = generateConnectorYaml(feed, date, flags);
  await applyManifest(connectorYaml);
  console.log(`  Created Connector ${feed}-${date}`);

  // Create Archiver CR
  const archiverYaml = generateArchiverYaml(feed, date, flags);
  await applyManifest(archiverYaml);
  console.log(`  Created Archiver ${feed}-${date}`);

  if (flags.wait !== false) {
    console.log("  Waiting for ready...");
    await waitForReady(feed, date);
  }

  console.log(`Trading day ${date} is ACTIVE`);
}

async function endDay(flags: DayFlags): Promise<void> {
  const env = flags._[2] as string;
  const date = flags.date || new Date().toISOString().split("T")[0];

  if (!env) {
    console.error("Usage: ssmd day end <env> [--date YYYY-MM-DD]");
    Deno.exit(1);
  }

  const feed = env.split("-")[0];
  const name = `${feed}-${date}`;

  console.log(`Ending trading day ${date}...`);

  // Delete CRs (finalizers handle cleanup)
  await kubectl(["delete", "archiver", name, "-n", "ssmd", "--ignore-not-found"]);
  console.log(`  Deleted Archiver ${name}`);

  await kubectl(["delete", "connector", name, "-n", "ssmd", "--ignore-not-found"]);
  console.log(`  Deleted Connector ${name}`);

  console.log(`Trading day ${date} is COMPLETE`);
}

async function statusDay(flags: DayFlags): Promise<void> {
  const env = flags._[2] as string;

  if (!env) {
    // Show all
    await listDays(flags);
    return;
  }

  const feed = env.split("-")[0];

  console.log(`Environment: ${env}`);

  // Get connectors for this feed
  const connectors = await kubectl([
    "get", "connector", "-n", "ssmd",
    "-l", `ssmd.io/feed=${feed}`,
    "-o", "jsonpath={range .items[*]}{.metadata.name} {.spec.date} {.status.phase} {.status.messagesPublished}\\n{end}"
  ]);

  console.log("\nConnectors:");
  for (const line of connectors.split("\n").filter(Boolean)) {
    const [name, date, phase, msgs] = line.split(" ");
    console.log(`  ${name}: ${phase} (${msgs || 0} messages)`);
  }

  // Get archivers for this feed (using labels since feed is in labels not spec)
  const archivers = await kubectl([
    "get", "archiver", "-n", "ssmd",
    "-l", `ssmd.io/feed=${feed}`,
    "-o", "jsonpath={range .items[*]}{.metadata.name} {.spec.date} {.status.phase} {.status.messagesArchived} {.status.bytesWritten}\\n{end}"
  ]);

  console.log("\nArchivers:");
  for (const line of archivers.split("\n").filter(Boolean)) {
    const [name, date, phase, msgs, bytes] = line.split(" ");
    console.log(`  ${name}: ${phase} (${msgs || 0} archived, ${formatBytes(parseInt(bytes) || 0)})`);
  }
}

async function listDays(_flags: DayFlags): Promise<void> {
  console.log("Active trading days:\n");
  console.log("NAME               DATE        CONNECTOR  ARCHIVER");
  console.log("----               ----        ---------  --------");

  // Get connectors
  const connectors = await kubectl([
    "get", "connector", "-n", "ssmd",
    "-o", "jsonpath={range .items[*]}{.metadata.name} {.spec.date} {.status.phase}\\n{end}"
  ]).catch(() => "");

  // Get archivers
  const archivers = await kubectl([
    "get", "archiver", "-n", "ssmd",
    "-o", "jsonpath={range .items[*]}{.metadata.name} {.spec.date} {.status.phase}\\n{end}"
  ]).catch(() => "");

  // Merge by name
  const days = new Map<string, { date?: string; connector?: string; archiver?: string }>();

  for (const line of connectors.split("\n").filter(Boolean)) {
    const [name, date, phase] = line.split(" ");
    if (!days.has(name)) days.set(name, {});
    days.get(name)!.date = date;
    days.get(name)!.connector = phase;
  }

  for (const line of archivers.split("\n").filter(Boolean)) {
    const [name, date, phase] = line.split(" ");
    if (!days.has(name)) days.set(name, {});
    days.get(name)!.date = date;
    days.get(name)!.archiver = phase;
  }

  if (days.size === 0) {
    console.log("(no active trading days)");
    return;
  }

  for (const [name, status] of days) {
    console.log(`${name.padEnd(18)} ${(status.date || "-").padEnd(11)} ${(status.connector || "-").padEnd(10)} ${status.archiver || "-"}`);
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

async function applyManifest(yaml: string): Promise<void> {
  const cmd = new Deno.Command("kubectl", {
    args: ["apply", "-f", "-"],
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  });

  const child = cmd.spawn();
  const writer = child.stdin.getWriter();
  await writer.write(new TextEncoder().encode(yaml));
  await writer.close();

  const { code, stderr } = await child.output();
  if (code !== 0) {
    throw new Error(`kubectl apply failed: ${new TextDecoder().decode(stderr)}`);
  }
}

// Exported for testing
export function generateConnectorYaml(feed: string, date: string, flags: DayFlags): string {
  const image = flags["connector-image"] || "ghcr.io/aaronwald/ssmd-connector:0.4.7";

  return `apiVersion: ssmd.ssmd.io/v1alpha1
kind: Connector
metadata:
  name: ${feed}-${date}
  namespace: ssmd
  labels:
    ssmd.io/feed: ${feed}
    ssmd.io/date: "${date}"
spec:
  feed: ${feed}
  date: "${date}"
  image: ${image}
  transport:
    type: nats
    url: nats://nats.nats:4222
    stream: PROD_KALSHI
    subjectPrefix: prod.${feed}
  secretRef:
    name: ssmd-kalshi-credentials
    apiKeyField: api-key
    privateKeyField: private-key
`;
}

// Exported for testing
export function generateArchiverYaml(feed: string, date: string, flags: DayFlags): string {
  const image = flags["archiver-image"] || "ghcr.io/aaronwald/ssmd-archiver:0.4.8";

  // Note: Archiver CRD uses source.filter instead of feed field
  return `apiVersion: ssmd.ssmd.io/v1alpha1
kind: Archiver
metadata:
  name: ${feed}-${date}
  namespace: ssmd
  labels:
    ssmd.io/feed: ${feed}
    ssmd.io/date: "${date}"
spec:
  date: "${date}"
  image: ${image}
  source:
    type: nats
    url: nats://nats.nats:4222
    stream: PROD_KALSHI
    consumer: archiver-${date}
    filter: "prod.${feed}.json.>"
  storage:
    local:
      path: /data/ssmd
      pvcName: ssmd-archiver-data-${date}
      pvcSize: 10Gi
  rotation:
    maxFileAge: "15m"
`;
}

async function waitForReady(feed: string, date: string, timeoutMs = 60000): Promise<void> {
  const name = `${feed}-${date}`;
  const start = Date.now();

  while (Date.now() - start < timeoutMs) {
    const connectorPhase = await kubectl([
      "get", "connector", name, "-n", "ssmd",
      "-o", "jsonpath={.status.phase}"
    ]).catch(() => "");

    const archiverPhase = await kubectl([
      "get", "archiver", name, "-n", "ssmd",
      "-o", "jsonpath={.status.phase}"
    ]).catch(() => "");

    if (connectorPhase === "Running" && archiverPhase === "Running") {
      console.log("  Connector ready");
      console.log("  Archiver ready");
      return;
    }

    await new Promise(r => setTimeout(r, 2000));
  }

  throw new Error("Timeout waiting for components to be ready");
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
}

function printDayHelp(): void {
  console.log("Usage: ssmd day <command> [options]");
  console.log("");
  console.log("Commands:");
  console.log("  start <env>    Start a trading day (creates Connector & Archiver CRs)");
  console.log("  end <env>      End a trading day (deletes CRs)");
  console.log("  status [env]   Show trading day status");
  console.log("  list           List all active trading days");
  console.log("");
  console.log("Options:");
  console.log("  --date YYYY-MM-DD    Trading date (default: today)");
  console.log("  --connector-image    Override connector image");
  console.log("  --archiver-image     Override archiver image");
  console.log("  --no-wait            Don't wait for ready status");
  console.log("");
  console.log("Examples:");
  console.log("  ssmd day start kalshi-prod");
  console.log("  ssmd day start kalshi-prod --date 2026-01-05");
  console.log("  ssmd day status kalshi-prod");
  console.log("  ssmd day end kalshi-prod --date 2026-01-05");
}
