import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import type { Signal, SignalResult, ComposerDecision } from "../../src/momentum/signals/types.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { SpreadTightening } from "../../src/momentum/signals/spread-tightening.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTicker(ticker: string, ts: number, price: number, yesBid: number, yesAsk: number): MarketRecord {
  return { type: "ticker", ticker, ts, price, yes_bid: yesBid, yes_ask: yesAsk, volume: 0, dollar_volume: 0 };
}

Deno.test("SignalResult has required fields", () => {
  const result: SignalResult = {
    name: "test",
    score: 0.5,
    confidence: 0.8,
    reason: "test reason",
  };
  assertEquals(result.name, "test");
  assertEquals(result.score, 0.5);
  assertEquals(result.confidence, 0.8);
});

Deno.test("ComposerDecision has required fields", () => {
  const decision: ComposerDecision = {
    enter: true,
    side: "yes",
    price: 55,
    score: 0.7,
    signals: [{ name: "test", score: 0.5, confidence: 0.8, reason: "r" }],
  };
  assertEquals(decision.enter, true);
  assertEquals(decision.side, "yes");
  assertEquals(decision.price, 55);
});

Deno.test("SpreadTightening: no signal without enough spread data", () => {
  const signal = new SpreadTightening({ spreadWindowMinutes: 5, narrowingThreshold: 0.5, weight: 1.0 });
  const state = new MarketState("T1");
  state.update(makeTicker("T1", 1000, 50, 48, 52));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("SpreadTightening: no signal when spread stays constant", () => {
  const signal = new SpreadTightening({ spreadWindowMinutes: 5, narrowingThreshold: 0.5, weight: 1.0 });
  const state = new MarketState("T1");
  for (let i = 0; i < 10; i++) {
    state.update(makeTicker("T1", 1000 + i * 30, 50, 48, 52));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("SpreadTightening: positive score when spread narrows with upward midpoint shift", () => {
  const signal = new SpreadTightening({ spreadWindowMinutes: 5, narrowingThreshold: 0.5, weight: 1.0 });
  const state = new MarketState("T1");
  for (let i = 0; i < 8; i++) {
    state.update(makeTicker("T1", 1000 + i * 30, 50, 47, 53));
  }
  state.update(makeTicker("T1", 1240, 54, 53, 55));
  state.update(makeTicker("T1", 1270, 54, 53, 55));

  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true);
  assertEquals(result.confidence > 0, true);
  assertEquals(result.name, "spread-tightening");
});

Deno.test("SpreadTightening: negative score when spread narrows with downward midpoint shift", () => {
  const signal = new SpreadTightening({ spreadWindowMinutes: 5, narrowingThreshold: 0.5, weight: 1.0 });
  const state = new MarketState("T1");
  for (let i = 0; i < 8; i++) {
    state.update(makeTicker("T1", 1000 + i * 30, 50, 47, 53));
  }
  state.update(makeTicker("T1", 1240, 46, 45, 47));
  state.update(makeTicker("T1", 1270, 46, 45, 47));

  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true);
});
