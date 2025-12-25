// ssmd-agent/test/integration.ts
import { assertEquals } from "https://deno.land/std@0.208.0/assert/mod.ts";
import { OrderBookBuilder } from "../src/state/orderbook.ts";
import { loadSkills } from "../src/agent/skills.ts";

Deno.test("OrderBookBuilder calculates spread", () => {
  const builder = new OrderBookBuilder();

  builder.update({
    type: "orderbook",
    ticker: "INXD-25001",
    ts: 1735084800000,
    yes_bid: 0.45,
    yes_ask: 0.55,
  });

  const state = builder.getState();
  assertEquals(state.ticker, "INXD-25001");
  assertEquals(state.bestBid, 0.45);
  assertEquals(state.bestAsk, 0.55);
  assertEquals(state.spread, 0.1);
});

Deno.test("OrderBookBuilder ignores non-orderbook messages", () => {
  const builder = new OrderBookBuilder();

  builder.update({
    type: "trade",
    ticker: "INXD-25001",
    ts: 1735084800000,
    price: 0.50,
  });

  const state = builder.getState();
  assertEquals(state.ticker, "");
  assertEquals(state.spread, 0);
});

Deno.test("OrderBookBuilder spreadPercent calculation", () => {
  const builder = new OrderBookBuilder();

  builder.update({
    type: "orderbook",
    ticker: "TEST",
    ts: 1735084800000,
    yes_bid: 0.40,
    yes_ask: 0.50,
  });

  const state = builder.getState();
  // spread = 0.10, spreadPercent = 0.10 / 0.50 = 0.20
  assertEquals(state.spreadPercent, 0.2);
});

Deno.test("Skills loader finds skills", async () => {
  const skills = await loadSkills();
  const names = skills.map((s) => s.name);

  assertEquals(names.includes("explore-data"), true);
  assertEquals(names.includes("monitor-spread"), true);
  assertEquals(names.includes("interpret-backtest"), true);
  assertEquals(names.includes("custom-signal"), true);
});

Deno.test("Skills have description and content", async () => {
  const skills = await loadSkills();

  for (const skill of skills) {
    assertEquals(typeof skill.name, "string");
    assertEquals(typeof skill.description, "string");
    assertEquals(typeof skill.content, "string");
    assertEquals(skill.content.length > 0, true);
  }
});
