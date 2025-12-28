// CLI command router
import { parse } from "https://deno.land/std@0.224.0/flags/mod.ts";

export async function run(args: string[]): Promise<void> {
  const flags = parse(args, {
    string: ["_"],
    boolean: ["help", "version"],
    alias: { h: "help", v: "version" },
  });

  const command = flags._[0] as string;

  if (flags.version) {
    console.log("ssmd 1.0.0");
    return;
  }

  if (flags.help || !command) {
    printHelp();
    return;
  }

  switch (command) {
    case "version":
      console.log("ssmd 1.0.0");
      break;

    case "agent":
      // Launch the existing agent REPL
      await import("../../cli.ts");
      break;

    default:
      console.error(`Unknown command: ${command}`);
      console.log("");
      printHelp();
      Deno.exit(1);
  }
}

function printHelp(): void {
  console.log("ssmd - Market data CLI and agent");
  console.log("");
  console.log("USAGE:");
  console.log("  ssmd <command> [options]");
  console.log("");
  console.log("COMMANDS:");
  console.log("  init              Initialize exchanges directory");
  console.log("  feed              Manage feed configurations");
  console.log("  secmaster         Security master database operations");
  console.log("  backtest          Run signal backtests");
  console.log("  agent             Start interactive agent REPL");
  console.log("");
  console.log("OPTIONS:");
  console.log("  -h, --help        Show this help message");
  console.log("  -v, --version     Show version");
}
