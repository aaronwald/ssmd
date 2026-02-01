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
    minContracts: z.number().default(1),
    maxContracts: z.number().default(200),
    drawdownHaltPercent: z.number().default(10),
  }).default({}),

  positions: z.object({
    takeProfitCents: z.number().default(5),
    stopLossCents: z.number().default(5),
    timeStopMinutes: z.number().default(15),
    minPriceCents: z.number().default(20),
    maxPriceCents: z.number().default(80),
    cooldownSeconds: z.number().default(300),
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
    meanReversion: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(1.0),
      anchorWindowMinutes: z.number().default(5),
      deviationThresholdCents: z.number().default(5),
      maxDeviationCents: z.number().default(12),
      recentWindowSec: z.number().default(60),
      stallWindowSec: z.number().default(15),
      minRecentChangeCents: z.number().default(2),
      minTrades: z.number().default(3),
    }).default({}),
    volatilitySqueeze: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(0.8),
      squeezeWindowMinutes: z.number().default(5),
      compressionThreshold: z.number().default(0.4),
      expansionThreshold: z.number().default(1.5),
      minBaselineStdDev: z.number().default(0.5),
      maxExpansionRatio: z.number().default(4.0),
      minSnapshots: z.number().default(10),
    }).default({}),
    priceMomentum: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(0.6),
      shortWindowSec: z.number().default(60),
      midWindowSec: z.number().default(180),
      longWindowSec: z.number().default(300),
      minTotalMoveCents: z.number().default(4),
      maxAccelRatio: z.number().default(3.0),
      minEntryPrice: z.number().default(40),
      maxEntryPrice: z.number().default(60),
      minTrades: z.number().default(5),
    }).default({}),
    tradeImbalance: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(1.0),
      windowSec: z.number().default(120),
      minTrades: z.number().default(8),
      imbalanceThreshold: z.number().default(0.65),
      sustainedWindowSec: z.number().default(60),
      sustainedThreshold: z.number().default(0.60),
    }).default({}),
    tradeConcentration: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(1.0),
      windowSec: z.number().default(120),
      minTrades: z.number().default(5),
      concentrationThreshold: z.number().default(0.15),
    }).default({}),
    flowAsymmetry: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(1.0),
      windowSec: z.number().default(120),
      minTrades: z.number().default(6),
      asymmetryThreshold: z.number().default(2),
    }).default({}),
    spreadVelocity: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(0.8),
      windowSec: z.number().default(30),
      minSnapshots: z.number().default(5),
      velocityThreshold: z.number().default(0.1),
    }).default({}),
  }).default({}),

  composer: z.object({
    entryThreshold: z.number().default(0.15),
    minSignals: z.number().default(1),
    maxSlippageCents: z.number().default(10),
  }).default({}),

  reporting: z.object({
    summaryIntervalMinutes: z.number().default(5),
    publishToNats: z.boolean().default(false),
    debug: z.boolean().default(false),
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
