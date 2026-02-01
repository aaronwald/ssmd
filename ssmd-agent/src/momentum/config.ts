import { z } from "zod";
import { parse as parseYaml } from "https://deno.land/std@0.224.0/yaml/mod.ts";

export const MomentumConfigSchema = z.object({
  nats: z.object({
    url: z.string().default("nats://localhost:4222"),
    stream: z.string().default("PROD_KALSHI_SPORTS"),
    filter: z.string().optional(),
  }),

  activation: z.object({
    dollarVolume: z.number().default(100000),
    windowMinutes: z.number().default(10),
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
    minPriceCents: z.number().default(20),
    maxPriceCents: z.number().default(80),
    cooldownMinutes: z.number().default(5),
  }).default({}),

  fees: z.object({
    defaultMakerPerContract: z.number().default(0),
    defaultTakerPerContract: z.number().default(2),
  }).default({}),

  marketClose: z.object({
    noEntryBufferMinutes: z.number().default(5),
    forceExitBufferMinutes: z.number().default(2),
  }).default({}),

  signals: z.object({
    spreadTightening: z.object({
      enabled: z.boolean().default(true),
      weight: z.number().default(1.0),
      spreadWindowMinutes: z.number().default(5),
      narrowingThreshold: z.number().default(0.5),
    }).default({}),
    volumeOnset: z.object({
      enabled: z.boolean().default(true),
      weight: z.number().default(1.0),
      recentWindowSec: z.number().default(30),
      baselineWindowMinutes: z.number().default(5),
      onsetMultiplier: z.number().default(1.5),
    }).default({}),
  }).default({}),

  composer: z.object({
    entryThreshold: z.number().default(0.5),
    minSignals: z.number().default(2),
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
