import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MomentumConfigSchema, DEFAULT_CONFIG } from "../../src/momentum/config.ts";

Deno.test("MomentumConfigSchema validates default config", () => {
  const result = MomentumConfigSchema.safeParse(DEFAULT_CONFIG);
  assertEquals(result.success, true);
});

Deno.test("MomentumConfigSchema applies defaults for minimal config", () => {
  const minimal = {
    nats: { url: "nats://localhost:4222", stream: "TEST" },
  };
  const result = MomentumConfigSchema.parse(minimal);
  assertEquals(result.portfolio.startingBalance, 500);
  assertEquals(result.portfolio.tradeSize, 100);
  assertEquals(result.portfolio.minContracts, 1);
  assertEquals(result.portfolio.maxContracts, 200);
  assertEquals(result.portfolio.drawdownHaltPercent, 10);
  assertEquals(result.positions.takeProfitCents, 5);
  assertEquals(result.positions.stopLossCents, 5);
  assertEquals(result.positions.timeStopMinutes, 15);
  assertEquals(result.activation.dollarVolume, 100000);
  assertEquals(result.activation.windowMinutes, 10);
  assertEquals(result.positions.minPriceCents, 20);
  assertEquals(result.positions.maxPriceCents, 80);
  assertEquals(result.positions.cooldownSeconds, 300);
  // Signal defaults
  assertEquals(result.signals.spreadTightening.enabled, true);
  assertEquals(result.signals.spreadTightening.weight, 1.0);
  assertEquals(result.signals.volumeOnset.enabled, true);
  assertEquals(result.signals.volumeOnset.weight, 1.0);
  assertEquals(result.composer.entryThreshold, 0.15);
  assertEquals(result.composer.minSignals, 1);
  assertEquals(result.reporting.debug, false);
});

Deno.test("MomentumConfigSchema allows overrides", () => {
  const custom = {
    nats: { url: "nats://prod:4222", stream: "PROD_KALSHI_SPORTS", filter: "prod.kalshi.sports.>" },
    portfolio: { startingBalance: 1000, tradeSize: 200, minContracts: 50, maxContracts: 150, drawdownHaltPercent: 20 },
    positions: { takeProfitCents: 10, stopLossCents: 3, timeStopMinutes: 10 },
    signals: { spreadTightening: { weight: 2.0 }, volumeOnset: { enabled: false } },
    composer: { entryThreshold: 0.7 },
    reporting: { debug: true },
  };
  const result = MomentumConfigSchema.parse(custom);
  assertEquals(result.portfolio.startingBalance, 1000);
  assertEquals(result.portfolio.minContracts, 50);
  assertEquals(result.portfolio.maxContracts, 150);
  assertEquals(result.positions.takeProfitCents, 10);
  assertEquals(result.signals.spreadTightening.weight, 2.0);
  assertEquals(result.signals.volumeOnset.enabled, false);
  assertEquals(result.composer.entryThreshold, 0.7);
  assertEquals(result.reporting.debug, true);
});

Deno.test("MomentumConfigSchema backward compat with old tradeSize config", () => {
  const oldConfig = {
    nats: { url: "nats://localhost:4222", stream: "TEST" },
    portfolio: { startingBalance: 500, tradeSize: 100 },
    composer: { entryThreshold: 0.5, minSignals: 2 },
  };
  const result = MomentumConfigSchema.parse(oldConfig);
  assertEquals(result.portfolio.tradeSize, 100);
  assertEquals(result.portfolio.minContracts, 1);
  assertEquals(result.portfolio.maxContracts, 200);
  assertEquals(result.composer.entryThreshold, 0.5);
  assertEquals(result.composer.minSignals, 2);
});
