import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { loadSignal, type LoadedSignal } from "../../src/backtest/loader.ts";
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";

Deno.test("loadSignal loads signal metadata from code", async () => {
  const tmpDir = await Deno.makeTempDir();
  const signalDir = join(tmpDir, "test-signal");
  await Deno.mkdir(signalDir, { recursive: true });

  // Create a signal file
  const signalCode = `
export const signal = {
  id: "test-signal",
  name: "Test Signal",
  requires: ["orderbook"],
  evaluate(state) { return state.orderbook.spread > 5; },
  payload(state) { return { spread: state.orderbook.spread }; },
};
`;
  await Deno.writeTextFile(join(signalDir, "signal.ts"), signalCode);

  const loaded = await loadSignal(signalDir);

  assertEquals(loaded.id, "test-signal");
  assertEquals(loaded.name, "Test Signal");
  assertEquals(loaded.requires, ["orderbook"]);
  assertEquals(loaded.path, join(signalDir, "signal.ts"));
  assertEquals(loaded.manifest, null);

  await Deno.remove(tmpDir, { recursive: true });
});

Deno.test("loadSignal loads manifest if present", async () => {
  const tmpDir = await Deno.makeTempDir();
  const signalDir = join(tmpDir, "signal-with-manifest");
  await Deno.mkdir(signalDir, { recursive: true });

  // Create signal file
  const signalCode = `
export const signal = {
  id: "manifest-test",
  requires: ["orderbook"],
  evaluate(state) { return true; },
  payload(state) { return {}; },
};
`;
  await Deno.writeTextFile(join(signalDir, "signal.ts"), signalCode);

  // Create manifest
  const manifestContent = `
feed: kalshi
dates:
  - "2025-12-25"
  - "2025-12-26"
`;
  await Deno.writeTextFile(join(signalDir, "backtest.yaml"), manifestContent);

  const loaded = await loadSignal(signalDir);

  assertEquals(loaded.id, "manifest-test");
  assertEquals(loaded.manifest?.feed, "kalshi");
  assertEquals(loaded.manifest?.dates, ["2025-12-25", "2025-12-26"]);

  await Deno.remove(tmpDir, { recursive: true });
});

Deno.test("loadSignal handles signal with date_range manifest", async () => {
  const tmpDir = await Deno.makeTempDir();
  const signalDir = join(tmpDir, "range-signal");
  await Deno.mkdir(signalDir, { recursive: true });

  // Create signal file
  const signalCode = `
export const signal = {
  id: "range-test",
  requires: ["orderbook", "priceHistory"],
  evaluate(state) { return false; },
  payload(state) { return null; },
};
`;
  await Deno.writeTextFile(join(signalDir, "signal.ts"), signalCode);

  // Create manifest with date_range
  const manifestContent = `
feed: kalshi
date_range:
  from: "2025-12-01"
  to: "2025-12-15"
sample_limit: 1000
`;
  await Deno.writeTextFile(join(signalDir, "backtest.yaml"), manifestContent);

  const loaded = await loadSignal(signalDir);

  assertEquals(loaded.manifest?.feed, "kalshi");
  assertEquals(loaded.manifest?.date_range?.from, "2025-12-01");
  assertEquals(loaded.manifest?.date_range?.to, "2025-12-15");
  assertEquals(loaded.manifest?.sample_limit, 1000);

  await Deno.remove(tmpDir, { recursive: true });
});

Deno.test("loadSignal extracts multiple requires", async () => {
  const tmpDir = await Deno.makeTempDir();
  const signalDir = join(tmpDir, "multi-requires");
  await Deno.mkdir(signalDir, { recursive: true });

  const signalCode = `
export const signal = {
  id: "multi-req",
  requires: ["orderbook", "priceHistory", "volumeProfile"],
  evaluate(state) { return true; },
  payload(state) { return {}; },
};
`;
  await Deno.writeTextFile(join(signalDir, "signal.ts"), signalCode);

  const loaded = await loadSignal(signalDir);

  assertEquals(loaded.requires, ["orderbook", "priceHistory", "volumeProfile"]);

  await Deno.remove(tmpDir, { recursive: true });
});
