import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { VolumeSpikeMomentum } from "../../src/momentum/models/volume-spike.ts";
import { TradeFlowImbalance } from "../../src/momentum/models/trade-flow.ts";
import { PriceAcceleration } from "../../src/momentum/models/price-acceleration.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTicker(ticker: string, ts: number, price: number, volume: number, dollarVolume: number): MarketRecord {
  return { type: "ticker", ticker, ts, price, volume, dollar_volume: dollarVolume, yes_bid: price - 1, yes_ask: price + 1 };
}

function makeTrade(ticker: string, ts: number, price: number, count: number, side: string): MarketRecord {
  return { type: "trade", ticker, ts, price, count, side };
}

// --- Volume Spike ---

Deno.test("VolumeSpike: no signal without baseline", () => {
  const model = new VolumeSpikeMomentum({ spikeMultiplier: 3, spikeWindowMinutes: 1, baselineWindowMinutes: 10, minPriceMoveCents: 3 });
  const state = new MarketState("T1");
  state.update(makeTicker("T1", 1000, 50, 0, 0));
  state.update(makeTicker("T1", 1060, 53, 1000, 500));
  const signal = model.evaluate(state);
  assertEquals(signal, null);
});

Deno.test("VolumeSpike: fires on volume surge with price move", () => {
  const model = new VolumeSpikeMomentum({ spikeMultiplier: 3, spikeWindowMinutes: 1, baselineWindowMinutes: 10, minPriceMoveCents: 3 });
  const state = new MarketState("T1");

  // Build 10 min baseline: ~100 dollar vol per minute
  state.update(makeTicker("T1", 1000, 40, 0, 0));
  for (let i = 1; i <= 10; i++) {
    state.update(makeTicker("T1", 1000 + i * 60, 40, i * 100, i * 100));
  }

  // Spike: 500 dollar vol in 1 minute (5x baseline) + price move of +5
  state.update(makeTicker("T1", 1000 + 11 * 60, 45, 1500, 1500));

  const signal = model.evaluate(state);
  assertEquals(signal !== null, true);
  assertEquals(signal!.side, "yes");
});

// --- Trade Flow Imbalance ---

Deno.test("TradeFlow: no signal with balanced flow", () => {
  const model = new TradeFlowImbalance({ dominanceThreshold: 0.70, windowMinutes: 2, minTrades: 5, minPriceMoveCents: 2 });
  const state = new MarketState("T1");
  state.update(makeTicker("T1", 1000, 50, 0, 0));
  for (let i = 0; i < 5; i++) {
    state.update(makeTrade("T1", 1001 + i, 50, 10, i % 2 === 0 ? "yes" : "no"));
  }
  state.update(makeTicker("T1", 1010, 50, 100, 50));
  const signal = model.evaluate(state);
  assertEquals(signal, null);
});

Deno.test("TradeFlow: fires on heavy YES flow with price up", () => {
  const model = new TradeFlowImbalance({ dominanceThreshold: 0.70, windowMinutes: 2, minTrades: 5, minPriceMoveCents: 2 });
  const state = new MarketState("T1");
  state.update(makeTicker("T1", 1000, 48, 0, 0));
  // 8 yes trades, 1 no trade
  for (let i = 0; i < 8; i++) {
    state.update(makeTrade("T1", 1001 + i, 50 + i, 10, "yes"));
  }
  state.update(makeTrade("T1", 1010, 49, 5, "no"));
  state.update(makeTicker("T1", 1011, 52, 100, 50));
  const signal = model.evaluate(state);
  assertEquals(signal !== null, true);
  assertEquals(signal!.side, "yes");
});

// --- Price Acceleration ---

Deno.test("PriceAcceleration: no signal when not accelerating", () => {
  const model = new PriceAcceleration({ accelerationMultiplier: 2, shortWindowMinutes: 1, longWindowMinutes: 5, minShortRateCentsPerMin: 2, minLongMoveCents: 3 });
  const state = new MarketState("T1");
  // Constant rate: 1 cent/min over 5 minutes
  state.update(makeTicker("T1", 1000, 40, 0, 0));
  state.update(makeTicker("T1", 1060, 41, 100, 50));
  state.update(makeTicker("T1", 1120, 42, 200, 100));
  state.update(makeTicker("T1", 1180, 43, 300, 150));
  state.update(makeTicker("T1", 1240, 44, 400, 200));
  state.update(makeTicker("T1", 1300, 45, 500, 250));
  const signal = model.evaluate(state);
  assertEquals(signal, null);
});

Deno.test("PriceAcceleration: fires when short window rate >> long window rate", () => {
  const model = new PriceAcceleration({ accelerationMultiplier: 2, shortWindowMinutes: 1, longWindowMinutes: 5, minShortRateCentsPerMin: 2, minLongMoveCents: 3 });
  const state = new MarketState("T1");
  // Slow start, then acceleration
  state.update(makeTicker("T1", 1000, 40, 0, 0));
  state.update(makeTicker("T1", 1060, 41, 100, 50));
  state.update(makeTicker("T1", 1120, 42, 200, 100));
  state.update(makeTicker("T1", 1180, 43, 300, 150));
  state.update(makeTicker("T1", 1240, 44, 400, 200));
  state.update(makeTicker("T1", 1300, 50, 500, 250));  // +6/min acceleration!

  const signal = model.evaluate(state);
  assertEquals(signal !== null, true);
  assertEquals(signal!.side, "yes");
});
