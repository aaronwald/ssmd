import { loadMomentumConfig, MomentumConfigSchema } from "../../momentum/config.ts";
import { runMomentum } from "../../momentum/runner.ts";

export async function handleMomentum(
  subcommand: string,
  flags: Record<string, unknown>,
): Promise<void> {
  switch (subcommand) {
    case "run":
      await handleRun(flags);
      break;
    default:
      console.log("ssmd momentum - Paper trading momentum models");
      console.log("");
      console.log("USAGE:");
      console.log("  ssmd momentum run [options]");
      console.log("");
      console.log("OPTIONS:");
      console.log("  --config <path>     Config file (YAML)");
      console.log("  --stream <name>     NATS stream (default: PROD_KALSHI_SPORTS)");
      console.log("  --balance <amount>  Starting balance in dollars (default: 500)");
      console.log("  --nats-url <url>    NATS URL (default: nats://localhost:4222)");
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
