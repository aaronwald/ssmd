import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import type { Signal, SignalResult, ComposerDecision } from "../../src/momentum/signals/types.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { SpreadTightening } from "../../src/momentum/signals/spread-tightening.ts";
import { VolumeOnset } from "../../src/momentum/signals/volume-onset.ts";
import { Composer } from "../../src/momentum/signals/composer.ts";
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

// --- VolumeOnset tests ---

function makeTickerWithVolume(ticker: string, ts: number, price: number, volume: number, dollarVolume: number, yesBid: number, yesAsk: number): MarketRecord {
  return { type: "ticker", ticker, ts, price, volume, dollar_volume: dollarVolume, yes_bid: yesBid, yes_ask: yesAsk };
}

function makeTrade(ticker: string, ts: number, price: number, count: number, side: string): MarketRecord {
  return { type: "trade", ticker, ts, price, count, side };
}

Deno.test("VolumeOnset: no signal without baseline data", () => {
  const signal = new VolumeOnset({ recentWindowSec: 30, baselineWindowMinutes: 5, onsetMultiplier: 1.5, weight: 1.0 });
  const state = new MarketState("T1");
  state.update(makeTickerWithVolume("T1", 1000, 50, 0, 0, 48, 52));
  state.update(makeTickerWithVolume("T1", 1030, 50, 100, 50, 48, 52));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("VolumeOnset: no signal when volume is at baseline rate", () => {
  const signal = new VolumeOnset({ recentWindowSec: 30, baselineWindowMinutes: 5, onsetMultiplier: 1.5, weight: 1.0 });
  const state = new MarketState("T1");
  let cumVol = 0;
  let cumDol = 0;
  state.update(makeTickerWithVolume("T1", 1000, 50, cumVol, cumDol, 48, 52));
  for (let i = 1; i <= 10; i++) {
    cumVol += 200;
    cumDol += 100;
    state.update(makeTickerWithVolume("T1", 1000 + i * 30, 50, cumVol, cumDol, 48, 52));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("VolumeOnset: fires on volume burst with YES-dominant trades", () => {
  const signal = new VolumeOnset({ recentWindowSec: 30, baselineWindowMinutes: 5, onsetMultiplier: 1.5, weight: 1.0 });
  const state = new MarketState("T1");
  let cumVol = 0;
  let cumDol = 0;
  state.update(makeTickerWithVolume("T1", 1000, 50, cumVol, cumDol, 48, 52));
  for (let i = 1; i <= 10; i++) {
    cumVol += 200;
    cumDol += 100;
    state.update(makeTickerWithVolume("T1", 1000 + i * 30, 50, cumVol, cumDol, 48, 52));
  }
  cumVol += 600;
  cumDol += 300;
  state.update(makeTickerWithVolume("T1", 1330, 52, cumVol, cumDol, 50, 54));
  state.update(makeTrade("T1", 1315, 52, 20, "yes"));
  state.update(makeTrade("T1", 1320, 53, 15, "yes"));
  state.update(makeTrade("T1", 1325, 51, 5, "no"));

  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true);
  assertEquals(result.confidence > 0, true);
  assertEquals(result.name, "volume-onset");
});

Deno.test("VolumeOnset: negative score with NO-dominant trades", () => {
  const signal = new VolumeOnset({ recentWindowSec: 30, baselineWindowMinutes: 5, onsetMultiplier: 1.5, weight: 1.0 });
  const state = new MarketState("T1");
  let cumVol = 0;
  let cumDol = 0;
  state.update(makeTickerWithVolume("T1", 1000, 50, cumVol, cumDol, 48, 52));
  for (let i = 1; i <= 10; i++) {
    cumVol += 200;
    cumDol += 100;
    state.update(makeTickerWithVolume("T1", 1000 + i * 30, 50, cumVol, cumDol, 48, 52));
  }
  cumVol += 600;
  cumDol += 300;
  state.update(makeTickerWithVolume("T1", 1330, 48, cumVol, cumDol, 46, 50));
  state.update(makeTrade("T1", 1315, 48, 20, "no"));
  state.update(makeTrade("T1", 1320, 47, 15, "no"));
  state.update(makeTrade("T1", 1325, 49, 5, "yes"));

  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true);
});

// --- Composer tests ---

class StubSignal implements Signal {
  readonly name: string;
  private result: SignalResult;
  constructor(name: string, score: number, confidence: number) {
    this.name = name;
    this.result = { name, score, confidence, reason: `stub ${name}` };
  }
  evaluate(_state: MarketState): SignalResult {
    return this.result;
  }
}

Deno.test("Composer: no entry when signals disagree on direction", () => {
  const composer = new Composer(
    [new StubSignal("a", 0.5, 0.8), new StubSignal("b", -0.5, 0.8)],
    [1.0, 1.0],
    { entryThreshold: 0.3, minSignals: 2 },
  );
  const state = new MarketState("T1");
  state.update({ type: "ticker", ticker: "T1", ts: 1000, price: 50, yes_bid: 48, yes_ask: 52 } as MarketRecord);
  const decision = composer.evaluate(state);
  assertEquals(decision.enter, false);
});

Deno.test("Composer: no entry when fewer than minSignals agree", () => {
  const composer = new Composer(
    [new StubSignal("a", 0.5, 0.8), new StubSignal("b", 0, 0)],
    [1.0, 1.0],
    { entryThreshold: 0.3, minSignals: 2 },
  );
  const state = new MarketState("T1");
  state.update({ type: "ticker", ticker: "T1", ts: 1000, price: 50, yes_bid: 48, yes_ask: 52 } as MarketRecord);
  const decision = composer.evaluate(state);
  assertEquals(decision.enter, false);
});

Deno.test("Composer: enters YES when signals converge positive above threshold", () => {
  const composer = new Composer(
    [new StubSignal("a", 0.6, 0.9), new StubSignal("b", 0.5, 0.8)],
    [1.0, 1.0],
    { entryThreshold: 0.3, minSignals: 2 },
  );
  const state = new MarketState("T1");
  state.update({ type: "ticker", ticker: "T1", ts: 1000, price: 50, yes_bid: 48, yes_ask: 52 } as MarketRecord);
  const decision = composer.evaluate(state);
  assertEquals(decision.enter, true);
  assertEquals(decision.side, "yes");
  assertEquals(decision.price, 52);
  assertEquals(decision.signals.length, 2);
});

Deno.test("Composer: enters NO when signals converge negative above threshold", () => {
  const composer = new Composer(
    [new StubSignal("a", -0.6, 0.9), new StubSignal("b", -0.5, 0.8)],
    [1.0, 1.0],
    { entryThreshold: 0.3, minSignals: 2 },
  );
  const state = new MarketState("T1");
  state.update({ type: "ticker", ticker: "T1", ts: 1000, price: 50, yes_bid: 48, yes_ask: 52, no_bid: 48, no_ask: 52 } as MarketRecord);
  const decision = composer.evaluate(state);
  assertEquals(decision.enter, true);
  assertEquals(decision.side, "no");
  assertEquals(decision.price, 48);
});

Deno.test("Composer: respects signal weights", () => {
  const composer = new Composer(
    [new StubSignal("a", 0.8, 1.0), new StubSignal("b", 0.2, 1.0)],
    [0.1, 2.0],
    { entryThreshold: 0.5, minSignals: 2 },
  );
  const state = new MarketState("T1");
  state.update({ type: "ticker", ticker: "T1", ts: 1000, price: 50, yes_bid: 48, yes_ask: 52 } as MarketRecord);
  const decision = composer.evaluate(state);
  // Weighted sum = (0.8 * 1.0 * 0.1 + 0.2 * 1.0 * 2.0) = 0.08 + 0.4 = 0.48, below 0.5
  assertEquals(decision.enter, false);
});
