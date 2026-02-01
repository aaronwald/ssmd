import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { TradeConcentration } from "../../src/momentum/signals/trade-concentration.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTrade(ticker: string, ts: number, side: string, count: number, price: number): MarketRecord {
  return { type: "trade", ticker, ts, side, count, price };
}

const defaultConfig = {
  windowSec: 120,
  minTrades: 5,
  concentrationThreshold: 0.15,
  weight: 1.0,
};

Deno.test("TradeConcentration: no signal with insufficient trades", () => {
  const signal = new TradeConcentration(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 10, 50));
  state.update(makeTrade("TEST", 101, "yes", 10, 50));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeConcentration: no signal when trades are evenly distributed", () => {
  const signal = new TradeConcentration(defaultConfig);
  const state = new MarketState("TEST");
  // 10 trades of equal size — HHI = 1/10 = 0.10, below threshold 0.15
  for (let i = 0; i < 10; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 50));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeConcentration: fires when one large trade dominates YES side", () => {
  const signal = new TradeConcentration(defaultConfig);
  const state = new MarketState("TEST");
  // 1 large YES trade + 4 small NO trades
  state.update(makeTrade("TEST", 100, "yes", 50, 55));
  state.update(makeTrade("TEST", 101, "no", 2, 45));
  state.update(makeTrade("TEST", 102, "no", 2, 45));
  state.update(makeTrade("TEST", 103, "no", 2, 45));
  state.update(makeTrade("TEST", 104, "no", 2, 45));
  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true, "should be positive (YES direction)");
  assertEquals(result.confidence > 0, true);
  assertEquals(result.name, "trade-concentration");
});

Deno.test("TradeConcentration: fires negative when large trade dominates NO side", () => {
  const signal = new TradeConcentration(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "no", 50, 45));
  state.update(makeTrade("TEST", 101, "yes", 2, 55));
  state.update(makeTrade("TEST", 102, "yes", 2, 55));
  state.update(makeTrade("TEST", 103, "yes", 2, 55));
  state.update(makeTrade("TEST", 104, "yes", 2, 55));
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should be negative (NO direction)");
});

Deno.test("TradeConcentration: score magnitude scales with concentration", () => {
  const signal = new TradeConcentration(defaultConfig);

  // Moderate concentration
  const state1 = new MarketState("TEST");
  state1.update(makeTrade("TEST", 100, "yes", 20, 55));
  for (let i = 1; i < 6; i++) {
    state1.update(makeTrade("TEST", 100 + i, "yes", 5, 55));
  }
  const r1 = signal.evaluate(state1);

  // High concentration
  const state2 = new MarketState("TEST");
  state2.update(makeTrade("TEST", 100, "yes", 100, 55));
  for (let i = 1; i < 6; i++) {
    state2.update(makeTrade("TEST", 100 + i, "yes", 1, 55));
  }
  const r2 = signal.evaluate(state2);

  assertEquals(r2.score > r1.score, true, "higher concentration should produce higher score");
});
