import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { FlowAsymmetry } from "../../src/momentum/signals/flow-asymmetry.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTrade(ticker: string, ts: number, side: string, count: number, price: number): MarketRecord {
  return { type: "trade", ticker, ts, side, count, price };
}

const defaultConfig = {
  windowSec: 120,
  minTrades: 6,
  asymmetryThreshold: 2,
  weight: 1.0,
};

Deno.test("FlowAsymmetry: no signal with insufficient trades", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 5, 55));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("FlowAsymmetry: no signal when trades only on one side", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  for (let i = 0; i < 8; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 55));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("FlowAsymmetry: no signal when prices are symmetric", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  // YES trades at 55, NO trades at 45 → 100-45=55, asymmetry = 55-55 = 0
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 55));
  }
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 104 + i, "no", 5, 45));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("FlowAsymmetry: positive when YES buyers pay above implied", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  // YES buyers at 58, NO buyers at 45 → implied YES from NO = 100-45=55
  // Asymmetry = 58 - 55 = 3, above threshold
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 58));
  }
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 104 + i, "no", 5, 45));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true, "should be positive (YES conviction)");
  assertEquals(result.name, "flow-asymmetry");
});

Deno.test("FlowAsymmetry: negative when NO buyers show conviction", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  // YES buyers at 55, NO buyers at 42 → implied YES from NO = 100-42=58
  // Asymmetry = 55 - 58 = -3, negative = NO conviction
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 55));
  }
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 104 + i, "no", 5, 42));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should be negative (NO conviction)");
});
