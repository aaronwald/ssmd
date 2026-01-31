import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { parseMomentumRecord } from "../../src/momentum/parse.ts";

Deno.test("parseMomentumRecord parses ticker message", () => {
  const raw = {
    type: "ticker",
    sid: 1,
    msg: {
      market_ticker: "KXNFL-SUPER-KC",
      yes_bid: 48,
      yes_ask: 52,
      last_price: 50,
      volume: 100000,
      dollar_volume: 50000,
      open_interest: 5000,
      ts: 1700000000,
    },
  };
  const record = parseMomentumRecord(raw);
  assertEquals(record?.type, "ticker");
  assertEquals(record?.ticker, "KXNFL-SUPER-KC");
  assertEquals(record?.yes_bid, 48);
  assertEquals(record?.yes_ask, 52);
  assertEquals(record?.price, 50);
  assertEquals(record?.volume, 100000);
  assertEquals(record?.dollar_volume, 50000);
  assertEquals(record?.ts, 1700000000);
});

Deno.test("parseMomentumRecord parses trade message with side and count", () => {
  const raw = {
    type: "trade",
    sid: 1,
    msg: {
      market_ticker: "KXNFL-SUPER-KC",
      price: 50,
      count: 10,
      side: "yes",
      ts: 1700000000,
    },
  };
  const record = parseMomentumRecord(raw);
  assertEquals(record?.type, "trade");
  assertEquals(record?.ticker, "KXNFL-SUPER-KC");
  assertEquals(record?.price, 50);
  assertEquals(record?.count, 10);
  assertEquals(record?.side, "yes");
  assertEquals(record?.ts, 1700000000);
});

Deno.test("parseMomentumRecord returns null for missing msg", () => {
  const raw = { type: "ticker" };
  const record = parseMomentumRecord(raw);
  assertEquals(record, null);
});
