import { assertEquals, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  SweepSpecSchema,
  generateConfigs,
  applyOverride,
  generateConfigId,
} from "../../../src/momentum/sweep/spec.ts";

Deno.test("applyOverride: sets top-level field", () => {
  const obj = { a: 1, b: 2 };
  applyOverride(obj, "a", 10);
  assertEquals(obj.a, 10);
});

Deno.test("applyOverride: sets nested field via dot notation", () => {
  const obj = { signals: { tradeImbalance: { windowSec: 120 } } };
  applyOverride(obj, "signals.tradeImbalance.windowSec", 60);
  assertEquals(obj.signals.tradeImbalance.windowSec, 60);
});

Deno.test("applyOverride: creates intermediate objects if missing", () => {
  const obj: Record<string, unknown> = {};
  applyOverride(obj, "signals.tradeImbalance.enabled", true);
  assertEquals((obj as any).signals.tradeImbalance.enabled, true);
});

Deno.test("generateConfigId: produces readable short id from params", () => {
  const params = {
    "signals.tradeImbalance.imbalanceThreshold": 0.80,
    "signals.tradeImbalance.windowSec": 120,
    "composer.minSignals": 2,
  };
  const id = generateConfigId(params);
  assertEquals(typeof id, "string");
  assertEquals(id.length > 0, true);
  assertEquals(id, generateConfigId(params));
});

Deno.test("generateConfigId: different params produce different ids", () => {
  const a = generateConfigId({ "composer.minSignals": 1 });
  const b = generateConfigId({ "composer.minSignals": 2 });
  assertEquals(a !== b, true);
});

Deno.test("generateConfigs: cartesian product of 2x2 = 4 configs", () => {
  const base = { signals: { tradeImbalance: { windowSec: 120, minTrades: 5 } } };
  const parameters = {
    "signals.tradeImbalance.windowSec": [60, 120],
    "signals.tradeImbalance.minTrades": [5, 10],
  };
  const configs = generateConfigs(base, parameters);
  assertEquals(configs.length, 4);

  const ids = configs.map(c => c.configId);
  assertEquals(new Set(ids).size, 4);

  const windowValues = configs.map(c => (c.config as any).signals.tradeImbalance.windowSec);
  assertEquals(windowValues.sort((a, b) => a - b), [60, 60, 120, 120]);
});

Deno.test("generateConfigs: single param = N configs", () => {
  const base = { composer: { minSignals: 1 } };
  const parameters = { "composer.minSignals": [1, 2, 3] };
  const configs = generateConfigs(base, parameters);
  assertEquals(configs.length, 3);
});

Deno.test("generateConfigs: preserves base config values not in parameters", () => {
  const base = { portfolio: { startingBalance: 500 }, composer: { minSignals: 1 } };
  const parameters = { "composer.minSignals": [1, 2] };
  const configs = generateConfigs(base, parameters);
  for (const c of configs) {
    assertEquals((c.config as any).portfolio.startingBalance, 500);
  }
});

Deno.test("generateConfigs: configs are independent (no shared references)", () => {
  const base = { signals: { tradeImbalance: { windowSec: 120 } } };
  const parameters = { "signals.tradeImbalance.windowSec": [60, 120] };
  const configs = generateConfigs(base, parameters);
  (configs[0].config as any).signals.tradeImbalance.windowSec = 999;
  assertEquals((configs[1].config as any).signals.tradeImbalance.windowSec !== 999, true);
});

Deno.test("SweepSpecSchema: validates valid spec", () => {
  const spec = SweepSpecSchema.parse({
    name: "test-sweep",
    base: "experiments/deployed.yaml",
    parameters: {
      "composer.minSignals": [1, 2],
    },
    dateRange: { from: "2026-01-16", to: "2026-01-26" },
  });
  assertEquals(spec.name, "test-sweep");
  assertEquals(spec.maxParallel, 5);
});

Deno.test("SweepSpecSchema: rejects empty parameters", () => {
  assertThrows(() => {
    SweepSpecSchema.parse({
      name: "test",
      base: "foo.yaml",
      parameters: {},
      dateRange: { from: "2026-01-16", to: "2026-01-26" },
    });
  });
});
