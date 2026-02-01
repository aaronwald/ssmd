import { loadMomentumConfig, MomentumConfigSchema } from "../../momentum/config.ts";
import { runMomentum } from "../../momentum/runner.ts";
import { runMomentumBacktest } from "../../momentum/backtest.ts";

export async function handleMomentum(
  subcommand: string,
  flags: Record<string, unknown>,
): Promise<void> {
  switch (subcommand) {
    case "run":
      await handleRun(flags);
      break;
    case "backtest":
      await handleBacktest(flags);
      break;
    default:
      console.log("ssmd momentum - Paper trading momentum models");
      console.log("");
      console.log("USAGE:");
      console.log("  ssmd momentum run [options]");
      console.log("  ssmd momentum backtest [options]");
      console.log("");
      console.log("RUN OPTIONS:");
      console.log("  --config <path>     Config file (YAML)");
      console.log("  --stream <name>     NATS stream (default: PROD_KALSHI_SPORTS)");
      console.log("  --balance <amount>  Starting balance in dollars (default: 500)");
      console.log("  --nats-url <url>    NATS URL (default: nats://localhost:4222)");
      console.log("");
      console.log("BACKTEST OPTIONS:");
      console.log("  --config <path>       Config file (YAML) — same as live runner");
      console.log("  --from <YYYY-MM-DD>   Start date");
      console.log("  --to <YYYY-MM-DD>     End date");
      console.log("  --dates <d1,d2,...>   Specific dates (alternative to --from/--to)");
      console.log("  --bucket <name>       GCS bucket (default: ssmd-archive)");
      console.log("  --prefix <path>       GCS prefix (default: kalshi/sports)");
      Deno.exit(1);
  }
}

async function handleRun(flags: Record<string, unknown>): Promise<void> {
  const configPath = flags.config as string | undefined;

  let config;
  if (configPath) {
    config = await loadMomentumConfig(configPath);
  } else {
    const overrides: Record<string, unknown> = {
      nats: {
        url: (flags["nats-url"] as string) ?? "nats://localhost:4222",
        stream: (flags.stream as string) ?? "PROD_KALSHI_SPORTS",
        filter: flags.filter as string | undefined,
      },
    };

    if (flags.balance) {
      overrides.portfolio = { startingBalance: Number(flags.balance) };
    }

    config = MomentumConfigSchema.parse(overrides);
  }

  await runMomentum(config);
}

function generateDateRange(from: string, to: string): string[] {
  const dates: string[] = [];
  const start = new Date(from + "T00:00:00Z");
  const end = new Date(to + "T00:00:00Z");

  for (let d = start; d <= end; d.setUTCDate(d.getUTCDate() + 1)) {
    dates.push(d.toISOString().slice(0, 10));
  }

  return dates;
}

async function handleBacktest(flags: Record<string, unknown>): Promise<void> {
  const configPath = flags.config as string | undefined;
  if (!configPath) {
    console.error("Error: --config is required for backtest");
    Deno.exit(1);
  }

  const config = await loadMomentumConfig(configPath);

  // Resolve dates
  let dates: string[];
  const datesFlag = flags.dates as string | undefined;
  const fromFlag = flags.from as string | undefined;
  const toFlag = flags.to as string | undefined;

  if (datesFlag) {
    dates = datesFlag.split(",").map((d) => d.trim());
  } else if (fromFlag && toFlag) {
    dates = generateDateRange(fromFlag, toFlag);
  } else if (fromFlag) {
    dates = [fromFlag];
  } else {
    console.error("Error: --from/--to or --dates is required for backtest");
    Deno.exit(1);
  }

  const bucket = (flags.bucket as string) ?? "ssmd-archive";
  const prefix = (flags.prefix as string) ?? "kalshi/sports";

  await runMomentumBacktest({ config, dates, bucket, prefix });
}
