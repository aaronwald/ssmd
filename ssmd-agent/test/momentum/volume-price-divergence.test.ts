import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { VolumePriceDivergence } from "../../src/momentum/signals/volume-price-divergence.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTicker(ticker: string, ts: number, price: number, volume: number, dollarVolume: number): MarketRecord {
  return { type: "ticker", ticker, ts, price, yes_bid: price - 2, yes_ask: price + 2, volume, dollar_volume: dollarVolume };
}

function makeTrade(ticker: string, ts: number, side: string, count: number, price: number): MarketRecord {
  return { type: "trade", ticker, ts, side, count, price };
}

const defaultConfig = {
  windowSec: 60,
  baselineWindowSec: 300,
  volumeMultiplier: 2.0,
  maxPriceMoveCents: 2,
  minTrades: 5,
  weight: 0.8,
};

Deno.test("VolumePriceDivergence: no signal without baseline volume", () => {
  const signal = new VolumePriceDivergence(defaultConfig);
  const state = new MarketState("TEST");
  // Only recent data, no baseline established
  state.update(makeTicker("TEST", 100, 50, 1000, 50000));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("VolumePriceDivergence: no signal when volume is at baseline", () => {
  const signal = new VolumePriceDivergence(defaultConfig);
  const state = new MarketState("TEST");
  // Build baseline over 300s
  for (let i = 0; i < 10; i++) {
    state.update(makeTicker("TEST", 100 + i * 30, 50, 1000 + i * 100, 50000 + i * 5000));
  }
  // Recent volume at same rate (not elevated)
  state.update(makeTicker("TEST", 400, 50, 2100, 105000));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("VolumePriceDivergence: no signal when price already moved", () => {
  const signal = new VolumePriceDivergence(defaultConfig);
  const state = new MarketState("TEST");
  // Build baseline
  for (let i = 0; i < 10; i++) {
    state.update(makeTicker("TEST", 100 + i * 30, 50, 1000 + i * 100, 50000 + i * 5000));
  }
  // High volume BUT price moved 5 cents (above maxPriceMoveCents=2)
  state.update(makeTicker("TEST", 395, 50, 2000, 100000));
  state.update(makeTicker("TEST", 400, 55, 3000, 200000));
  // Add trades for direction
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("TEST", 395 + i, "yes", 5, 55));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("VolumePriceDivergence: fires when volume surges but price flat", () => {
  const signal = new VolumePriceDivergence(defaultConfig);
  const state = new MarketState("TEST");
  // Build baseline: steady volume
  for (let i = 0; i < 10; i++) {
    state.update(makeTicker("TEST", 100 + i * 30, 50, 1000 + i * 100, 50000 + i * 5000));
  }
  // Recent: volume spike, price stays at 50
  state.update(makeTicker("TEST", 395, 50, 2000, 100000));
  state.update(makeTicker("TEST", 400, 50, 2500, 150000));
  // Add YES-dominant trades for direction
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("TEST", 395 + i, "yes", 10, 50));
  }
  const result = signal.evaluate(state);
  assertEquals(result.name, "volume-price-divergence");
  // Direction depends on trade flow — YES dominant trades should give positive
  assertEquals(result.score > 0 || result.score < 0, true, "should fire a signal");
});
