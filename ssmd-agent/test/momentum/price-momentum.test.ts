import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { PriceMomentum } from "../../src/momentum/signals/price-momentum.ts";
import type { MarketRecord } from "../../src/state/types.ts";

const DEFAULT_CONFIG = {
  shortWindowSec: 60,
  midWindowSec: 180,
  longWindowSec: 300,
  minTotalMoveCents: 4,
  maxAccelRatio: 3.0,
  minEntryPrice: 40,
  maxEntryPrice: 60,
  minTrades: 5,
  weight: 0.6,
};

function makeTicker(ticker: string, ts: number, price: number, yesBid: number, yesAsk: number): MarketRecord {
  return { type: "ticker", ticker, ts, price, yes_bid: yesBid, yes_ask: yesAsk, volume: 0, dollar_volume: 0 };
}

function makeTrade(ticker: string, ts: number, price: number, count: number, side: string): MarketRecord {
  return { type: "trade", ticker, ts, price, count, side };
}

Deno.test("PriceMomentum: no signal with insufficient data", () => {
  const signal = new PriceMomentum(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  state.update(makeTicker("T1", 1000, 50, 48, 52));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("PriceMomentum: no signal when windows disagree on direction", () => {
  const signal = new PriceMomentum(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  const base = 1000;
  // Long window: down
  state.update(makeTicker("T1", base, 55, 53, 57));
  state.update(makeTicker("T1", base + 60, 53, 51, 55));
  state.update(makeTicker("T1", base + 120, 51, 49, 53));
  // Mid window: flat to up
  state.update(makeTicker("T1", base + 180, 50, 48, 52));
  state.update(makeTicker("T1", base + 240, 52, 50, 54));
  // Short window: up
  state.update(makeTicker("T1", base + 250, 53, 51, 55));
  state.update(makeTicker("T1", base + 290, 55, 53, 57));
  // Trades
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("T1", base + 250 + i * 5, 54, 10, "yes"));
  }

  const result = signal.evaluate(state);
  assertEquals(result.score, 0, "should not fire when windows disagree");
});

Deno.test("PriceMomentum: no signal when total move too small", () => {
  const signal = new PriceMomentum(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  const base = 1000;
  // Very gradual upward drift — less than 4c total
  state.update(makeTicker("T1", base, 49, 47, 51));
  state.update(makeTicker("T1", base + 60, 50, 48, 52));
  state.update(makeTicker("T1", base + 120, 50, 48, 52));
  state.update(makeTicker("T1", base + 180, 50, 48, 52));
  state.update(makeTicker("T1", base + 240, 51, 49, 53));
  state.update(makeTicker("T1", base + 300, 51, 49, 53));
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("T1", base + 250 + i * 5, 51, 10, "yes"));
  }

  const result = signal.evaluate(state);
  assertEquals(result.score, 0, "should not fire with < 4c total move");
});

Deno.test("PriceMomentum: no signal when decelerating", () => {
  const signal = new PriceMomentum(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  const base = 1000;
  // Fast long-window move, slow short-window move (decelerating)
  state.update(makeTicker("T1", base, 42, 40, 44));
  state.update(makeTicker("T1", base + 60, 45, 43, 47));
  state.update(makeTicker("T1", base + 120, 48, 46, 50));
  state.update(makeTicker("T1", base + 180, 50, 48, 52));
  state.update(makeTicker("T1", base + 240, 51, 49, 53));
  // Short window: barely moving
  state.update(makeTicker("T1", base + 260, 51, 49, 53));
  state.update(makeTicker("T1", base + 300, 52, 50, 54));
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("T1", base + 260 + i * 5, 52, 10, "yes"));
  }

  const result = signal.evaluate(state);
  assertEquals(result.score, 0, "should not fire when decelerating");
});

Deno.test("PriceMomentum: no signal when trade flow diverges", () => {
  const signal = new PriceMomentum(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  const base = 1000;
  // Strong upward momentum in price
  state.update(makeTicker("T1", base, 42, 40, 44));
  state.update(makeTicker("T1", base + 60, 44, 42, 46));
  state.update(makeTicker("T1", base + 120, 46, 44, 48));
  state.update(makeTicker("T1", base + 180, 48, 46, 50));
  state.update(makeTicker("T1", base + 240, 50, 48, 52));
  state.update(makeTicker("T1", base + 260, 52, 50, 54));
  state.update(makeTicker("T1", base + 300, 54, 52, 56));
  // But trade flow is NO-dominant (diverging)
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("T1", base + 260 + i * 5, 53, 10, "no"));
  }

  const result = signal.evaluate(state);
  assertEquals(result.score, 0, "should not fire when trade flow diverges from price direction");
});

Deno.test("PriceMomentum: no signal when price too high for YES momentum", () => {
  const signal = new PriceMomentum(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  const base = 1000;
  // Upward momentum but price > maxEntryPrice (60)
  state.update(makeTicker("T1", base, 58, 56, 60));
  state.update(makeTicker("T1", base + 60, 60, 58, 62));
  state.update(makeTicker("T1", base + 120, 62, 60, 64));
  state.update(makeTicker("T1", base + 180, 63, 61, 65));
  state.update(makeTicker("T1", base + 240, 64, 62, 66));
  state.update(makeTicker("T1", base + 260, 65, 63, 67));
  state.update(makeTicker("T1", base + 300, 66, 64, 68));
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("T1", base + 260 + i * 5, 66, 10, "yes"));
  }

  const result = signal.evaluate(state);
  assertEquals(result.score, 0, "should not fire when price above max entry price");
});

Deno.test("PriceMomentum: fires positive (YES) on sustained accelerating upward move", () => {
  const signal = new PriceMomentum(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  const base = 1000;
  // Steady acceleration upward within price band
  state.update(makeTicker("T1", base, 42, 40, 44));
  state.update(makeTicker("T1", base + 60, 43, 41, 45));
  state.update(makeTicker("T1", base + 120, 44, 42, 46));
  state.update(makeTicker("T1", base + 180, 46, 44, 48));
  state.update(makeTicker("T1", base + 240, 48, 46, 50));
  // Short window accelerates
  state.update(makeTicker("T1", base + 260, 50, 48, 52));
  state.update(makeTicker("T1", base + 280, 52, 50, 54));
  state.update(makeTicker("T1", base + 300, 54, 52, 56));
  // YES-dominant trade flow
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("T1", base + 260 + i * 7, 52, 10, "yes"));
  }

  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true, `expected positive score, got ${result.score}`);
  assertEquals(result.name, "price-momentum");
  assertEquals(result.confidence > 0, true);
});

Deno.test("PriceMomentum: fires negative (NO) on sustained accelerating downward move", () => {
  const signal = new PriceMomentum(DEFAULT_CONFIG);
  const state = new MarketState("T1");
  const base = 1000;
  // Steady acceleration downward within price band
  state.update(makeTicker("T1", base, 58, 56, 60));
  state.update(makeTicker("T1", base + 60, 57, 55, 59));
  state.update(makeTicker("T1", base + 120, 56, 54, 58));
  state.update(makeTicker("T1", base + 180, 54, 52, 56));
  state.update(makeTicker("T1", base + 240, 52, 50, 54));
  // Short window accelerates downward
  state.update(makeTicker("T1", base + 260, 50, 48, 52));
  state.update(makeTicker("T1", base + 280, 48, 46, 50));
  state.update(makeTicker("T1", base + 300, 46, 44, 48));
  // NO-dominant trade flow
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("T1", base + 260 + i * 7, 48, 10, "no"));
  }

  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, `expected negative score, got ${result.score}`);
  assertEquals(result.name, "price-momentum");
});
