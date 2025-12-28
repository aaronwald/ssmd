import { assertEquals, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketSchema, fromKalshiMarket, type KalshiMarket } from "../../../src/lib/types/market.ts";

Deno.test("MarketSchema validates valid market", () => {
  const market = {
    ticker: "KXBTC-24DEC31-T100K",
    event_ticker: "KXBTC-24DEC31",
    title: "Bitcoin > $100k on Dec 31?",
    status: "active",
    yes_bid: 45,
    yes_ask: 47,
    volume: 10000,
  };

  const result = MarketSchema.parse(market);
  assertEquals(result.ticker, "KXBTC-24DEC31-T100K");
  assertEquals(result.yes_bid, 45);
});

Deno.test("MarketSchema defaults volume fields to 0", () => {
  const market = {
    ticker: "TEST-MKT",
    event_ticker: "TEST-EVENT",
    title: "Test Market",
  };

  const result = MarketSchema.parse(market);
  assertEquals(result.volume, 0);
  assertEquals(result.volume_24h, 0);
  assertEquals(result.open_interest, 0);
});

Deno.test("MarketSchema defaults status to active", () => {
  const market = {
    ticker: "TEST-MKT",
    event_ticker: "TEST-EVENT",
    title: "Test Market",
  };

  const result = MarketSchema.parse(market);
  assertEquals(result.status, "active");
});

Deno.test("MarketSchema rejects empty ticker", () => {
  const market = {
    ticker: "",
    event_ticker: "TEST",
    title: "Test",
  };

  assertThrows(() => MarketSchema.parse(market));
});

Deno.test("MarketSchema allows null price fields", () => {
  const market = {
    ticker: "TEST-MKT",
    event_ticker: "TEST-EVENT",
    title: "Test Market",
    yes_bid: null,
    yes_ask: null,
    last_price: null,
  };

  const result = MarketSchema.parse(market);
  assertEquals(result.yes_bid, null);
  assertEquals(result.yes_ask, null);
});

Deno.test("fromKalshiMarket converts API response", () => {
  const kalshiMarket: KalshiMarket = {
    ticker: "KXBTC-24DEC31-T100K",
    event_ticker: "KXBTC-24DEC31",
    title: "Yes",
    subtitle: "Bitcoin > $100k",
    status: "active",
    yes_bid: 45,
    yes_ask: 47,
    no_bid: 53,
    no_ask: 55,
    last_price: 46,
    volume: 10000,
    volume_24h: 500,
    open_interest: 2000,
    close_time: "2024-12-31T23:59:59Z",
    can_close_early: false,
  };

  const market = fromKalshiMarket(kalshiMarket);

  assertEquals(market.ticker, "KXBTC-24DEC31-T100K");
  assertEquals(market.title, "Yes");
  assertEquals(market.yes_bid, 45);
  assertEquals(market.volume, 10000);
  // Extra fields should be stripped
  assertEquals("result" in market, false);
  assertEquals("can_close_early" in market, false);
});

Deno.test("fromKalshiMarket uses subtitle as fallback title", () => {
  const kalshiMarket: KalshiMarket = {
    ticker: "TEST-MKT",
    event_ticker: "TEST-EVENT",
    subtitle: "Fallback Title",
    status: "active",
  };

  const market = fromKalshiMarket(kalshiMarket);
  assertEquals(market.title, "Fallback Title");
});

Deno.test("fromKalshiMarket maps status correctly", () => {
  const activeMarket = fromKalshiMarket({
    ticker: "A",
    event_ticker: "E",
    status: "active",
  });
  assertEquals(activeMarket.status, "active");

  const closedMarket = fromKalshiMarket({
    ticker: "B",
    event_ticker: "E",
    status: "closed",
  });
  assertEquals(closedMarket.status, "closed");

  const settledMarket = fromKalshiMarket({
    ticker: "C",
    event_ticker: "E",
    status: "finalized",
  });
  assertEquals(settledMarket.status, "settled");
});
