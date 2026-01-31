import { z } from "zod";
import { parse as parseYaml } from "https://deno.land/std@0.224.0/yaml/mod.ts";

export const MomentumConfigSchema = z.object({
  nats: z.object({
    url: z.string().default("nats://localhost:4222"),
    stream: z.string().default("PROD_KALSHI_SPORTS"),
    filter: z.string().optional(),
  }),

  activation: z.object({
    dollarVolume: z.number().default(250000),
    windowMinutes: z.number().default(30),
  }).default({}),

  portfolio: z.object({
    startingBalance: z.number().default(500),
    tradeSize: z.number().default(100),
    drawdownHaltPercent: z.number().default(10),
  }).default({}),

  positions: z.object({
    takeProfitCents: z.number().default(5),
    stopLossCents: z.number().default(5),
    timeStopMinutes: z.number().default(15),
  }).default({}),

  fees: z.object({
    defaultMakerPerContract: z.number().default(0),
    defaultTakerPerContract: z.number().default(2),
  }).default({}),

  marketClose: z.object({
    noEntryBufferMinutes: z.number().default(5),
    forceExitBufferMinutes: z.number().default(2),
  }).default({}),

  models: z.object({
    volumeSpike: z.object({
      enabled: z.boolean().default(true),
      spikeMultiplier: z.number().default(3.0),
      spikeWindowMinutes: z.number().default(1),
      baselineWindowMinutes: z.number().default(10),
      minPriceMoveCents: z.number().default(3),
    }).default({}),
    tradeFlow: z.object({
      enabled: z.boolean().default(true),
      dominanceThreshold: z.number().default(0.70),
      windowMinutes: z.number().default(2),
      minTrades: z.number().default(5),
      minPriceMoveCents: z.number().default(2),
    }).default({}),
    priceAcceleration: z.object({
      enabled: z.boolean().default(true),
      accelerationMultiplier: z.number().default(2.0),
      shortWindowMinutes: z.number().default(1),
      longWindowMinutes: z.number().default(5),
      minShortRateCentsPerMin: z.number().default(2),
      minLongMoveCents: z.number().default(3),
    }).default({}),
  }).default({}),

  reporting: z.object({
    summaryIntervalMinutes: z.number().default(5),
    publishToNats: z.boolean().default(false),
  }).default({}),
});

export type MomentumConfig = z.infer<typeof MomentumConfigSchema>;

export const DEFAULT_CONFIG: MomentumConfig = MomentumConfigSchema.parse({
  nats: { url: "nats://localhost:4222", stream: "PROD_KALSHI_SPORTS" },
});

export async function loadMomentumConfig(configPath?: string): Promise<MomentumConfig> {
  if (configPath) {
    const content = await Deno.readTextFile(configPath);
    const raw = parseYaml(content);
    return MomentumConfigSchema.parse(raw);
  }
  return DEFAULT_CONFIG;
}
