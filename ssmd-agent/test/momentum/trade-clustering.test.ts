import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { TradeClustering } from "../../src/momentum/signals/trade-clustering.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTrade(ticker: string, ts: number, side: string, count: number, price: number): MarketRecord {
  return { type: "trade", ticker, ts, side, count, price };
}

const defaultConfig = {
  windowSec: 120,
  quietThresholdSec: 15,
  burstGapSec: 3,
  minBurstTrades: 4,
  weight: 1.0,
};

Deno.test("TradeClustering: no signal with insufficient trades", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 5, 50));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeClustering: no signal when trades are evenly spaced (no burst)", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  // Trades every 10 seconds — no quiet period before a burst
  for (let i = 0; i < 8; i++) {
    state.update(makeTrade("TEST", 100 + i * 10, "yes", 5, 50));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeClustering: fires on burst after quiet period", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  // One trade, then 20s quiet, then burst of 4 rapid YES trades
  state.update(makeTrade("TEST", 100, "no", 2, 50));
  // quiet gap: 100 → 121 = 21 seconds (> quietThresholdSec=15)
  state.update(makeTrade("TEST", 121, "yes", 10, 52));
  state.update(makeTrade("TEST", 122, "yes", 8, 53));
  state.update(makeTrade("TEST", 123, "yes", 12, 54));
  state.update(makeTrade("TEST", 124, "yes", 6, 53));
  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true, "should be positive (YES burst)");
  assertEquals(result.name, "trade-clustering");
});

Deno.test("TradeClustering: fires negative on NO burst", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 2, 50));
  // quiet gap
  state.update(makeTrade("TEST", 121, "no", 10, 48));
  state.update(makeTrade("TEST", 122, "no", 8, 47));
  state.update(makeTrade("TEST", 123, "no", 12, 46));
  state.update(makeTrade("TEST", 124, "no", 6, 47));
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should be negative (NO burst)");
});

Deno.test("TradeClustering: no signal when burst is too small", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 2, 50));
  // quiet gap, then only 3 trades (< minBurstTrades=4)
  state.update(makeTrade("TEST", 121, "yes", 10, 52));
  state.update(makeTrade("TEST", 122, "yes", 8, 53));
  state.update(makeTrade("TEST", 123, "yes", 12, 54));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeClustering: picks the most recent burst", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  // First burst (YES)
  state.update(makeTrade("TEST", 50, "no", 2, 50));
  state.update(makeTrade("TEST", 70, "yes", 10, 52));
  state.update(makeTrade("TEST", 71, "yes", 8, 53));
  state.update(makeTrade("TEST", 72, "yes", 12, 54));
  state.update(makeTrade("TEST", 73, "yes", 6, 53));
  // Second burst (NO) — more recent, with quiet period before
  state.update(makeTrade("TEST", 100, "no", 10, 48));
  state.update(makeTrade("TEST", 101, "no", 8, 47));
  state.update(makeTrade("TEST", 102, "no", 12, 46));
  state.update(makeTrade("TEST", 103, "no", 6, 47));
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should pick most recent burst (NO)");
});
