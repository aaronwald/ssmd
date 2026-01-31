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
  assertEquals(result.portfolio.drawdownHaltPercent, 10);
  assertEquals(result.positions.takeProfitCents, 5);
  assertEquals(result.positions.stopLossCents, 5);
  assertEquals(result.positions.timeStopMinutes, 15);
  assertEquals(result.activation.dollarVolume, 250000);
});

Deno.test("MomentumConfigSchema allows overrides", () => {
  const custom = {
    nats: { url: "nats://prod:4222", stream: "PROD_KALSHI_SPORTS", filter: "prod.kalshi.sports.>" },
    portfolio: { startingBalance: 1000, tradeSize: 200, drawdownHaltPercent: 20 },
    positions: { takeProfitCents: 10, stopLossCents: 3, timeStopMinutes: 10 },
  };
  const result = MomentumConfigSchema.parse(custom);
  assertEquals(result.portfolio.startingBalance, 1000);
  assertEquals(result.positions.takeProfitCents, 10);
});
