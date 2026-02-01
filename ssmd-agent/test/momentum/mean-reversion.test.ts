import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { MeanReversion } from "../../src/momentum/signals/mean-reversion.ts";
import type { MarketRecord } from "../../src/state/types.ts";

const DEFAULT_CONFIG = {
  anchorWindowMinutes: 5,
  deviationThresholdCents: 5,
  maxDeviationCents: 12,
  recentWindowSec: 60,
  stallWindowSec: 15,
  minRecentChangeCents: 2,
  minTrades: 3,
  weight: 1.0,
};

function makeTicker(ticker: string, ts: number, price: number, yesBid: number, yesAsk: number): MarketRecord {
  return { type: "ticker", ticker, ts, price, yes_bid: yesBid, yes_ask: yesAsk, volume: 0, dollar_volume: 0 };
}

function makeTrade(ticker: string, ts: number, price: number, count: number, side: string): MarketRecord {
  return { type: "trade", ticker, ts, price, count, side };
}

Deno.test("MeanReversion: no signal with insufficient data", () => {
  const signal = new MeanReversion(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  state.update(makeTicker("T1", 1000, 50, 48, 52));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("MeanReversion: no signal when deviation is below threshold", () => {
  const signal = new MeanReversion(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  // Build anchor around 50c
  for (let i = 0; i < 10; i++) {
    state.update(makeTicker("T1", 1000 + i * 30, 50, 48, 52));
  }
  // Price moves to 53 (3c deviation, below 5c threshold)
  state.update(makeTicker("T1", 1300, 53, 51, 55));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("MeanReversion: no signal when move is stale (no recent change)", () => {
  const signal = new MeanReversion(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  // Build anchor around 50c
  for (let i = 0; i < 8; i++) {
    state.update(makeTicker("T1", 1000 + i * 30, 50, 48, 52));
  }
  // Price at 58 but has been there for a while (no recent change)
  for (let i = 0; i < 5; i++) {
    state.update(makeTicker("T1", 1240 + i * 30, 58, 56, 60));
  }
  const result = signal.evaluate(state);
  // recentChange over 60s should be ~0 since price stable at 58
  assertEquals(result.score, 0);
});

Deno.test("MeanReversion: no signal when move is still extending (stall check fails)", () => {
  const signal = new MeanReversion(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  // Build anchor around 50c
  for (let i = 0; i < 8; i++) {
    state.update(makeTicker("T1", 1000 + i * 30, 50, 48, 52));
  }
  // Price moves up quickly and is STILL moving up (no stall)
  state.update(makeTicker("T1", 1250, 55, 53, 57));
  state.update(makeTicker("T1", 1260, 56, 54, 58));
  state.update(makeTicker("T1", 1270, 57, 55, 59));
  // Add trades for trade factor
  state.update(makeTrade("T1", 1260, 56, 10, "yes"));
  state.update(makeTrade("T1", 1265, 57, 10, "yes"));
  state.update(makeTrade("T1", 1270, 57, 10, "yes"));
  const result = signal.evaluate(state);
  // Deviation > threshold, recent change > min, but veryRecentChange still positive = still extending
  assertEquals(result.score, 0, "should not fire while move is still extending");
});

Deno.test("MeanReversion: fires negative (NO) when price overextended upward and stalled", () => {
  const signal = new MeanReversion(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  // Build anchor around 50c (midpoint of 48-52)
  for (let i = 0; i < 8; i++) {
    state.update(makeTicker("T1", 1000 + i * 30, 50, 48, 52));
  }
  // Sharp move up — starts within recentWindow (60s) from end
  state.update(makeTicker("T1", 1240, 52, 50, 54));
  state.update(makeTicker("T1", 1255, 55, 53, 57));
  state.update(makeTicker("T1", 1270, 57, 55, 59));
  // Then stall / slight pullback (veryRecentChange <= 0 within stallWindowSec=15)
  state.update(makeTicker("T1", 1285, 57, 55, 59));
  state.update(makeTicker("T1", 1300, 56, 54, 58));
  // Add trades
  state.update(makeTrade("T1", 1270, 57, 10, "yes"));
  state.update(makeTrade("T1", 1285, 56, 10, "no"));
  state.update(makeTrade("T1", 1300, 56, 10, "no"));

  const result = signal.evaluate(state);
  // lastPrice=56, anchor~50c, deviation=+6c > threshold 5c
  // recentChange over 60s: |56 - 52| = 4c > minRecentChangeCents 2c
  // veryRecentChange over 15s: 56 - 57 = -1 (stalled, deviation > 0 but change <= 0)
  assertEquals(result.score < 0, true, `expected negative score, got ${result.score}`);
  assertEquals(result.confidence > 0, true);
  assertEquals(result.name, "mean-reversion");
});

Deno.test("MeanReversion: fires positive (YES) when price overextended downward and stalled", () => {
  const signal = new MeanReversion(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  // Build anchor around 50c
  for (let i = 0; i < 8; i++) {
    state.update(makeTicker("T1", 1000 + i * 30, 50, 48, 52));
  }
  // Sharp move down — starts within recentWindow (60s) from end
  state.update(makeTicker("T1", 1240, 48, 46, 50));
  state.update(makeTicker("T1", 1255, 45, 43, 47));
  state.update(makeTicker("T1", 1270, 43, 41, 45));
  // Stall / bounce (veryRecentChange >= 0 within stallWindowSec=15)
  state.update(makeTicker("T1", 1285, 43, 41, 45));
  state.update(makeTicker("T1", 1300, 44, 42, 46));
  // Trades
  state.update(makeTrade("T1", 1270, 43, 10, "no"));
  state.update(makeTrade("T1", 1285, 44, 10, "yes"));
  state.update(makeTrade("T1", 1300, 44, 10, "yes"));

  const result = signal.evaluate(state);
  // lastPrice=44, anchor~50c, deviation=-6c, absDeviation=6c > threshold 5c
  // recentChange over 60s: |44 - 48| = 4c > minRecentChangeCents 2c
  // veryRecentChange over 15s: 44 - 43 = +1 (bounced, deviation < 0 but change >= 0)
  assertEquals(result.score > 0, true, `expected positive score, got ${result.score}`);
  assertEquals(result.confidence > 0, true);
});

Deno.test("MeanReversion: score magnitude scales with deviation", () => {
  const signal = new MeanReversion(DEFAULT_CONFIG);

  // Small deviation (~6c from anchor)
  const state1 = new MarketState("T1");
  for (let i = 0; i < 8; i++) {
    state1.update(makeTicker("T1", 1000 + i * 30, 50, 48, 52));
  }
  // Move up to 56-57c, pull back to 56c (lastPrice=56, anchor~50 → deviation~6c)
  state1.update(makeTicker("T1", 1240, 53, 51, 55));
  state1.update(makeTicker("T1", 1260, 56, 54, 58));
  state1.update(makeTicker("T1", 1280, 57, 55, 59));
  state1.update(makeTicker("T1", 1295, 57, 55, 59));
  state1.update(makeTicker("T1", 1300, 56, 54, 58));
  state1.update(makeTrade("T1", 1270, 57, 10, "yes"));
  state1.update(makeTrade("T1", 1290, 56, 10, "no"));
  state1.update(makeTrade("T1", 1300, 56, 10, "no"));
  const r1 = signal.evaluate(state1);

  // Large deviation (~10c from anchor)
  const state2 = new MarketState("T2");
  for (let i = 0; i < 8; i++) {
    state2.update(makeTicker("T2", 1000 + i * 30, 50, 48, 52));
  }
  // Move up to 61c, pull back to 60c (lastPrice=60, anchor~50 → deviation~10c)
  state2.update(makeTicker("T2", 1240, 55, 53, 57));
  state2.update(makeTicker("T2", 1260, 59, 57, 61));
  state2.update(makeTicker("T2", 1280, 61, 59, 63));
  state2.update(makeTicker("T2", 1295, 61, 59, 63));
  state2.update(makeTicker("T2", 1300, 60, 58, 62));
  state2.update(makeTrade("T2", 1270, 61, 10, "yes"));
  state2.update(makeTrade("T2", 1290, 60, 10, "no"));
  state2.update(makeTrade("T2", 1300, 60, 10, "no"));
  const r2 = signal.evaluate(state2);

  // Both should fire negative, larger deviation should have larger magnitude
  assertEquals(r1.score !== 0, true, `small deviation should fire, got score=${r1.score}`);
  assertEquals(r2.score !== 0, true, `large deviation should fire, got score=${r2.score}`);
  assertEquals(Math.abs(r2.score) > Math.abs(r1.score), true,
    `larger deviation should have larger magnitude: ${Math.abs(r2.score)} vs ${Math.abs(r1.score)}`);
});
