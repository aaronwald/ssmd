import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { SpreadVelocity } from "../../src/momentum/signals/spread-velocity.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTicker(ticker: string, ts: number, yesBid: number, yesAsk: number): MarketRecord {
  return { type: "ticker", ticker, ts, price: (yesBid + yesAsk) / 2, yes_bid: yesBid, yes_ask: yesAsk, volume: 0, dollar_volume: 0 };
}

const defaultConfig = {
  windowSec: 30,
  minSnapshots: 5,
  velocityThreshold: 0.1,
  weight: 0.8,
};

Deno.test("SpreadVelocity: no signal with insufficient snapshots", () => {
  const signal = new SpreadVelocity(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTicker("TEST", 100, 45, 55));
  state.update(makeTicker("TEST", 105, 46, 54));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("SpreadVelocity: no signal when spread is stable", () => {
  const signal = new SpreadVelocity(defaultConfig);
  const state = new MarketState("TEST");
  for (let i = 0; i < 6; i++) {
    state.update(makeTicker("TEST", 100 + i * 5, 45, 55));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("SpreadVelocity: fires when spread narrows rapidly with rising midpoint", () => {
  const signal = new SpreadVelocity(defaultConfig);
  const state = new MarketState("TEST");
  // Spread narrows from 10 to 2, midpoint rises (YES direction)
  state.update(makeTicker("TEST", 100, 45, 55)); // spread=10, mid=50
  state.update(makeTicker("TEST", 105, 47, 55)); // spread=8, mid=51
  state.update(makeTicker("TEST", 110, 49, 55)); // spread=6, mid=52
  state.update(makeTicker("TEST", 115, 51, 55)); // spread=4, mid=53
  state.update(makeTicker("TEST", 120, 53, 55)); // spread=2, mid=54
  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true, "should be positive (rising midpoint = YES)");
  assertEquals(result.name, "spread-velocity");
});

Deno.test("SpreadVelocity: fires negative when midpoint falls during narrowing", () => {
  const signal = new SpreadVelocity(defaultConfig);
  const state = new MarketState("TEST");
  // Spread narrows, midpoint falls (NO direction)
  state.update(makeTicker("TEST", 100, 45, 55)); // spread=10, mid=50
  state.update(makeTicker("TEST", 105, 45, 53)); // spread=8, mid=49
  state.update(makeTicker("TEST", 110, 45, 51)); // spread=6, mid=48
  state.update(makeTicker("TEST", 115, 45, 49)); // spread=4, mid=47
  state.update(makeTicker("TEST", 120, 45, 47)); // spread=2, mid=46
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should be negative (falling midpoint = NO)");
});

Deno.test("SpreadVelocity: confidence higher for more linear narrowing", () => {
  const signal = new SpreadVelocity(defaultConfig);
  // Linear narrowing with rising midpoint should have high R²
  const state = new MarketState("TEST");
  state.update(makeTicker("TEST", 100, 45, 55)); // spread=10, mid=50
  state.update(makeTicker("TEST", 105, 48, 54)); // spread=6, mid=51
  state.update(makeTicker("TEST", 110, 50, 54)); // spread=4, mid=52
  state.update(makeTicker("TEST", 115, 52, 54)); // spread=2, mid=53
  state.update(makeTicker("TEST", 120, 53, 54)); // spread=1, mid=53.5
  const result = signal.evaluate(state);
  assertEquals(result.confidence > 0.5, true, "linear narrowing should give high confidence");
});
