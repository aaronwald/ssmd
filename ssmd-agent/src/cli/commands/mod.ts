// CLI command router
import { parse } from "https://deno.land/std@0.224.0/flags/mod.ts";
import { getFeedsDir } from "../utils/paths.ts";
import { initExchanges } from "./init.ts";
import {
  listFeeds,
  showFeed,
  createFeed,
  printFeedList,
  printFeed,
  type CreateFeedOptions,
} from "./feed.ts";
import { handleBacktest } from "./backtest.ts";
import { handleSecmaster } from "./secmaster.ts";
import { handleFees } from "./fees.ts";
import { handleSignal } from "./signal.ts";
import { handleSignalDeploy } from "./signal-deploy.ts";
import { handleNotifierDeploy } from "./notifier-deploy.ts";
import { handleConnectorDeploy } from "./connector-deploy.ts";
import { handleArchiverDeploy } from "./archiver-deploy.ts";
import { handleDay } from "./day.ts";
import { handleStatus } from "./status.ts";

export async function run(args: string[]): Promise<void> {
  const flags = parse(args, {
    string: ["_", "type", "endpoint", "display-name", "auth-method", "dates", "from", "to", "sha", "feed", "limit", "source", "data", "nats-url", "stream", "subject", "date", "connector-image", "archiver-image", "namespace", "message", "destination", "tail"],
    boolean: ["help", "version", "allow-dirty", "no-wait", "events-only", "markets-only", "no-delete", "dry-run", "console", "wait", "follow"],
    alias: { h: "help", v: "version", t: "type", e: "endpoint", f: "follow" },
    default: { wait: true },
  });

  const command = flags._[0] as string;
  const subcommand = flags._[1] as string;

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

    case "init": {
      const path = flags._[1] as string | undefined;
      await initExchanges(path);
      console.log("\nInitialized exchanges directory.");
      break;
    }

    case "feed":
      await handleFeedCommand(subcommand, flags);
      break;

    case "backtest":
      await handleBacktest(subcommand, flags);
      break;

    case "secmaster":
      await handleSecmaster(subcommand, flags);
      break;

    case "fees":
      await handleFees(subcommand, flags);
      break;

    case "signal": {
      // CR management commands go to signal-deploy
      if (["deploy", "status", "logs", "delete"].includes(subcommand)) {
        await handleSignalDeploy(subcommand, flags);
      } else {
        // Local signal commands (list, run, subscribe)
        await handleSignal(subcommand, flags);
      }
      break;
    }

    case "day":
      await handleDay(subcommand, flags);
      break;

    case "notifier":
      await handleNotifierDeploy(subcommand, flags);
      break;

    case "connector":
      await handleConnectorDeploy(subcommand, flags);
      break;

    case "archiver":
      await handleArchiverDeploy(subcommand, flags);
      break;

    case "status":
      await handleStatus(flags);
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

async function handleFeedCommand(
  subcommand: string,
  flags: ReturnType<typeof parse>
): Promise<void> {
  const feedsDir = await getFeedsDir();

  switch (subcommand) {
    case "list":
    case undefined: {
      const feeds = await listFeeds(feedsDir);
      printFeedList(feeds);
      break;
    }

    case "show": {
      const name = flags._[2] as string;
      if (!name) {
        console.error("Usage: ssmd feed show <name>");
        Deno.exit(1);
      }
      const feed = await showFeed(feedsDir, name);
      if (!feed) {
        console.error(`Feed '${name}' not found`);
        Deno.exit(1);
      }
      printFeed(feed);
      break;
    }

    case "add":
    case "create": {
      const name = flags._[2] as string;
      if (!name || !flags.type) {
        console.error("Usage: ssmd feed add <name> --type <websocket|rest|multicast>");
        Deno.exit(1);
      }
      const options: CreateFeedOptions = {
        type: flags.type as CreateFeedOptions["type"],
        displayName: flags["display-name"] as string | undefined,
        endpoint: flags.endpoint as string | undefined,
        authMethod: flags["auth-method"] as string | undefined,
      };
      await createFeed(feedsDir, name, options);
      console.log(`Created feed: ${name}`);
      break;
    }

    default:
      console.error(`Unknown feed command: ${subcommand}`);
      console.log("Usage: ssmd feed [list|show|add]");
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
  console.log("  status            Show cluster status overview");
  console.log("  init              Initialize exchanges directory");
  console.log("  feed              Manage feed configurations");
  console.log("  secmaster         Security master database operations");
  console.log("  fees              Fee schedule database operations");
  console.log("  backtest          Run signal backtests");
  console.log("  signal            Run signals locally or manage Signal CRs");
  console.log("  notifier          Manage Notifier CRs in Kubernetes");
  console.log("  connector         Manage Connector CRs in Kubernetes");
  console.log("  archiver          Manage Archiver CRs in Kubernetes");
  console.log("  day               Manage trading day lifecycle");
  console.log("  agent             Start interactive agent REPL");
  console.log("");
  console.log("OPTIONS:");
  console.log("  -h, --help        Show this help message");
  console.log("  -v, --version     Show version");
}
