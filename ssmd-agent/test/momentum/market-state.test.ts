import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTicker(ticker: string, ts: number, price: number, volume: number, dollarVolume: number, yesBid?: number, yesAsk?: number): MarketRecord {
  return { type: "ticker", ticker, ts, price, volume, dollar_volume: dollarVolume, yes_bid: yesBid, yes_ask: yesAsk };
}

function makeTrade(ticker: string, ts: number, price: number, count: number, side: string): MarketRecord {
  return { type: "trade", ticker, ts, price, count, side };
}

Deno.test("MarketState tracks price from ticker messages", () => {
  const state = new MarketState("TEST-1");
  state.update(makeTicker("TEST-1", 1000, 50, 100, 50, 48, 52));
  assertEquals(state.lastPrice, 50);
  assertEquals(state.yesBid, 48);
  assertEquals(state.yesAsk, 52);
});

Deno.test("MarketState tracks volume activation", () => {
  const state = new MarketState("TEST-1");
  assertEquals(state.isActivated(250000, 30 * 60), false);
  state.update(makeTicker("TEST-1", 1000, 50, 0, 0));
  state.update(makeTicker("TEST-1", 1001, 50, 5000, 260000));
  assertEquals(state.isActivated(250000, 30 * 60), true);
});

Deno.test("MarketState tracks trade flow by side", () => {
  const state = new MarketState("TEST-1");
  state.update(makeTrade("TEST-1", 1000, 50, 100, "yes"));
  state.update(makeTrade("TEST-1", 1001, 51, 50, "yes"));
  state.update(makeTrade("TEST-1", 1002, 49, 30, "no"));

  const flow = state.getTradeFlow(2 * 60);
  assertEquals(flow.yesVolume, 150);
  assertEquals(flow.noVolume, 30);
  assertEquals(flow.totalTrades, 3);
});

Deno.test("MarketState computes volume rate over window", () => {
  const state = new MarketState("TEST-1");
  state.update(makeTicker("TEST-1", 1000, 50, 0, 0));
  state.update(makeTicker("TEST-1", 1060, 53, 500, 250));

  const rate = state.getVolumeRate(60);
  assertEquals(rate.dollarVolume, 250);
});

Deno.test("MarketState computes price change over window", () => {
  const state = new MarketState("TEST-1");
  state.update(makeTicker("TEST-1", 1000, 40, 0, 0));
  state.update(makeTicker("TEST-1", 1030, 43, 100, 50));
  state.update(makeTicker("TEST-1", 1060, 47, 200, 100));

  const change = state.getPriceChange(120);
  assertEquals(change, 7);
});

Deno.test("MarketState sliding window trims old data", () => {
  const state = new MarketState("TEST-1");
  state.update(makeTicker("TEST-1", 1000, 40, 0, 0));
  state.update(makeTicker("TEST-1", 2000, 50, 1000, 500));

  // 60s window should not include the old price point (1000s ago)
  const rate = state.getVolumeRate(60);
  assertEquals(rate.dollarVolume, 500);

  const change = state.getPriceChange(60);
  // ts=1000 is outside 60s window from ts=2000, so only one point → 0
  assertEquals(change, 0);
});

Deno.test("MarketState price rate of change detects acceleration", () => {
  const state = new MarketState("TEST-1");
  // 5 snapshots 60s apart
  state.update(makeTicker("TEST-1", 1000, 40, 0, 0));
  state.update(makeTicker("TEST-1", 1060, 42, 100, 50));
  state.update(makeTicker("TEST-1", 1120, 44, 200, 100));
  state.update(makeTicker("TEST-1", 1180, 47, 300, 150));
  state.update(makeTicker("TEST-1", 1240, 52, 400, 200));

  // Rate over 5min window: includes all points, (52-40)/4min = 3 cents/min
  const longRate = state.getPriceRatePerMinute(5 * 60);
  // Rate over 2min window: includes last ~2 points, faster rate
  const shortRate = state.getPriceRatePerMinute(2 * 60);

  assertEquals(shortRate > longRate, true);
});
