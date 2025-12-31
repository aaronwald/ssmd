import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { OrderBookBuilder } from "../../src/state/orderbook.ts";
import type { MarketRecord } from "../../src/state/types.ts";

// Helper to create a MarketRecord
function createRecord(
  overrides: Partial<MarketRecord> = {}
): MarketRecord {
  return {
    type: "orderbook",
    ticker: "TEST-TICKER",
    ts: Date.now(),
    ...overrides,
  };
}

Deno.test("OrderBookBuilder - initial state has zero values", () => {
  const builder = new OrderBookBuilder();
  const state = builder.getState();

  assertEquals(state.ticker, "");
  assertEquals(state.bestBid, 0);
  assertEquals(state.bestAsk, 0);
  assertEquals(state.spread, 0);
  assertEquals(state.spreadPercent, 0);
  assertEquals(state.lastUpdate, 0);
});

Deno.test("OrderBookBuilder - updates state from orderbook message", () => {
  const builder = new OrderBookBuilder();
  const record = createRecord({
    type: "orderbook",
    ticker: "INXD-25-B4000",
    yes_bid: 45,
    yes_ask: 55,
    ts: 1704067200000,
  });

  builder.update(record);
  const state = builder.getState();

  assertEquals(state.ticker, "INXD-25-B4000");
  assertEquals(state.bestBid, 45);
  assertEquals(state.bestAsk, 55);
  assertEquals(state.spread, 10);
  assertEquals(state.lastUpdate, 1704067200000);
});

Deno.test("OrderBookBuilder - updates state from ticker message", () => {
  const builder = new OrderBookBuilder();
  const record = createRecord({
    type: "ticker",
    ticker: "KXBTC-25-B100K",
    yes_bid: 30,
    yes_ask: 35,
    ts: 1704067200000,
  });

  builder.update(record);
  const state = builder.getState();

  assertEquals(state.ticker, "KXBTC-25-B100K");
  assertEquals(state.bestBid, 30);
  assertEquals(state.bestAsk, 35);
  assertEquals(state.spread, 5);
});

Deno.test("OrderBookBuilder - ignores non-orderbook/ticker messages", () => {
  const builder = new OrderBookBuilder();

  // First set some state
  builder.update(
    createRecord({ type: "orderbook", yes_bid: 50, yes_ask: 60 })
  );

  // Trade message should be ignored
  builder.update(
    createRecord({ type: "trade", yes_bid: 99, yes_ask: 99 })
  );

  const state = builder.getState();
  assertEquals(state.bestBid, 50);
  assertEquals(state.bestAsk, 60);
});

Deno.test("OrderBookBuilder - calculates spread correctly", () => {
  const builder = new OrderBookBuilder();

  builder.update(createRecord({ yes_bid: 20, yes_ask: 80 }));
  assertEquals(builder.getState().spread, 60);

  builder.update(createRecord({ yes_bid: 49, yes_ask: 51 }));
  assertEquals(builder.getState().spread, 2);

  // Tight spread
  builder.update(createRecord({ yes_bid: 50, yes_ask: 51 }));
  assertEquals(builder.getState().spread, 1);
});

Deno.test("OrderBookBuilder - calculates spread percent correctly", () => {
  const builder = new OrderBookBuilder();

  // 10/50 = 20%
  builder.update(createRecord({ yes_bid: 40, yes_ask: 50 }));
  assertEquals(builder.getState().spreadPercent, 0.2);

  // 25/100 = 25%
  builder.update(createRecord({ yes_bid: 75, yes_ask: 100 }));
  assertEquals(builder.getState().spreadPercent, 0.25);

  // 1/51 ≈ 1.96%
  builder.update(createRecord({ yes_bid: 50, yes_ask: 51 }));
  const state = builder.getState();
  assertEquals(state.spreadPercent.toFixed(4), (1 / 51).toFixed(4));
});

Deno.test("OrderBookBuilder - handles zero ask price (division by zero)", () => {
  const builder = new OrderBookBuilder();

  builder.update(createRecord({ yes_bid: 0, yes_ask: 0 }));
  const state = builder.getState();

  assertEquals(state.spreadPercent, 0); // Should not be NaN or Infinity
});

Deno.test("OrderBookBuilder - handles missing bid/ask values", () => {
  const builder = new OrderBookBuilder();

  // Record with no bid/ask values
  builder.update(createRecord({ type: "orderbook" }));
  const state = builder.getState();

  assertEquals(state.bestBid, 0);
  assertEquals(state.bestAsk, 0);
  assertEquals(state.spread, 0);
});

Deno.test("OrderBookBuilder - reset returns to initial state", () => {
  const builder = new OrderBookBuilder();

  // Set some state
  builder.update(
    createRecord({
      ticker: "SOME-TICKER",
      yes_bid: 40,
      yes_ask: 60,
      ts: 9999999,
    })
  );

  // Verify state was set
  assertEquals(builder.getState().ticker, "SOME-TICKER");

  // Reset
  builder.reset();
  const state = builder.getState();

  assertEquals(state.ticker, "");
  assertEquals(state.bestBid, 0);
  assertEquals(state.bestAsk, 0);
  assertEquals(state.lastUpdate, 0);
});

Deno.test("OrderBookBuilder - getState returns immutable copy", () => {
  const builder = new OrderBookBuilder();
  builder.update(createRecord({ yes_bid: 50, yes_ask: 60 }));

  const state1 = builder.getState();
  const state2 = builder.getState();

  // Should be equal but not the same object
  assertEquals(state1, state2);
  assertEquals(state1 === state2, false);

  // Mutating returned state should not affect builder
  (state1 as { bestBid: number }).bestBid = 999;
  assertEquals(builder.getState().bestBid, 50);
});

Deno.test("OrderBookBuilder - updates preserve latest timestamp", () => {
  const builder = new OrderBookBuilder();

  builder.update(createRecord({ ts: 1000 }));
  assertEquals(builder.getState().lastUpdate, 1000);

  builder.update(createRecord({ ts: 2000 }));
  assertEquals(builder.getState().lastUpdate, 2000);

  // Even older timestamps are stored (no max logic)
  builder.update(createRecord({ ts: 1500 }));
  assertEquals(builder.getState().lastUpdate, 1500);
});

Deno.test("OrderBookBuilder - has correct id", () => {
  const builder = new OrderBookBuilder();
  assertEquals(builder.id, "orderbook");
});

// Edge case tests for crossed book detection
Deno.test("OrderBookBuilder - crossed book (bid >= ask)", () => {
  const builder = new OrderBookBuilder();

  // Crossed book: bid equals ask
  builder.update(createRecord({ yes_bid: 50, yes_ask: 50 }));
  let state = builder.getState();
  assertEquals(state.spread, 0);
  assertEquals(state.spreadPercent, 0);

  // Crossed book: bid greater than ask (invalid market state)
  builder.update(createRecord({ yes_bid: 60, yes_ask: 40 }));
  state = builder.getState();
  assertEquals(state.spread, -20); // Negative spread indicates crossed book
  // Note: Current implementation doesn't flag this as invalid
});

Deno.test("OrderBookBuilder - handles sequential updates", () => {
  const builder = new OrderBookBuilder();

  // Simulate market moving
  builder.update(createRecord({ yes_bid: 45, yes_ask: 55 }));
  assertEquals(builder.getState().spread, 10);

  builder.update(createRecord({ yes_bid: 48, yes_ask: 52 }));
  assertEquals(builder.getState().spread, 4);

  builder.update(createRecord({ yes_bid: 50, yes_ask: 51 }));
  assertEquals(builder.getState().spread, 1);
});

Deno.test("OrderBookBuilder - handles extreme values", () => {
  const builder = new OrderBookBuilder();

  // Maximum possible prices (0-100 cents for Kalshi)
  builder.update(createRecord({ yes_bid: 99, yes_ask: 100 }));
  let state = builder.getState();
  assertEquals(state.spread, 1);
  assertEquals(state.spreadPercent, 0.01);

  // Minimum bid
  builder.update(createRecord({ yes_bid: 1, yes_ask: 99 }));
  state = builder.getState();
  assertEquals(state.spread, 98);
});
