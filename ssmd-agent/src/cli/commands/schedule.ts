// schedule.ts - Temporal schedule management commands

interface ScheduleFlags {
  _: (string | number)[];
  namespace?: string;
}

export async function handleSchedule(
  subcommand: string,
  flags: ScheduleFlags
): Promise<void> {
  const namespace = flags.namespace ?? "default";

  switch (subcommand) {
    case "list":
    case undefined:
      await listSchedules(namespace);
      break;
    case "describe": {
      const scheduleId = flags._[2] as string;
      if (!scheduleId) {
        console.error("Usage: ssmd schedule describe <schedule-id>");
        Deno.exit(1);
      }
      await describeSchedule(namespace, scheduleId);
      break;
    }
    default:
      console.error(`Unknown schedule command: ${subcommand}`);
      printScheduleHelp();
      Deno.exit(1);
  }
}

async function listSchedules(namespace: string): Promise<void> {
  console.log("Temporal Schedules\n");

  try {
    const output = await kubectl([
      "exec", "-n", "temporal", "deployment/temporal-server", "--",
      "temporal", "schedule", "list",
      "--namespace", namespace,
      "--address", "localhost:7233",
    ]);

    // Parse and format the output
    const lines = output.trim().split("\n");
    if (lines.length <= 1) {
      console.log("No schedules found.");
      return;
    }

    // Print header and data
    for (const line of lines) {
      console.log(line);
    }
  } catch (e) {
    console.error(`Failed to list schedules: ${e}`);
    Deno.exit(1);
  }
}

async function describeSchedule(namespace: string, scheduleId: string): Promise<void> {
  try {
    const output = await kubectl([
      "exec", "-n", "temporal", "deployment/temporal-server", "--",
      "temporal", "schedule", "describe",
      "--namespace", namespace,
      "--schedule-id", scheduleId,
      "--address", "localhost:7233",
    ]);

    console.log(output);
  } catch (e) {
    console.error(`Failed to describe schedule: ${e}`);
    Deno.exit(1);
  }
}

async function kubectl(args: string[]): Promise<string> {
  const cmd = new Deno.Command("kubectl", { args, stdout: "piped", stderr: "piped" });
  const { stdout, stderr, code } = await cmd.output();

  if (code !== 0) {
    const err = new TextDecoder().decode(stderr);
    throw new Error(err.trim());
  }

  return new TextDecoder().decode(stdout);
}

function printScheduleHelp(): void {
  console.log("Usage: ssmd schedule <command> [options]");
  console.log("");
  console.log("Commands:");
  console.log("  list                 List all Temporal schedules");
  console.log("  describe <id>        Show details of a specific schedule");
  console.log("");
  console.log("Options:");
  console.log("  --namespace <ns>     Temporal namespace (default: 'default')");
  console.log("");
  console.log("Examples:");
  console.log("  ssmd schedule list");
  console.log("  ssmd schedule describe ssmd-scale-down-weekly");
}
