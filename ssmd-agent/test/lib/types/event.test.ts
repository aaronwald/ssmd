import { assertEquals, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { EventSchema, fromKalshiEvent, type KalshiEvent } from "../../../src/lib/types/event.ts";

Deno.test("EventSchema validates valid event", () => {
  const event = {
    event_ticker: "KXBTC-24DEC31",
    title: "Bitcoin price on Dec 31, 2024",
    category: "Crypto",
    series_ticker: "KXBTC",
    strike_date: "2024-12-31",
    mutually_exclusive: true,
    status: "active",
  };

  const result = EventSchema.parse(event);
  assertEquals(result.event_ticker, "KXBTC-24DEC31");
  assertEquals(result.status, "active");
});

Deno.test("EventSchema defaults mutually_exclusive to false", () => {
  const event = {
    event_ticker: "TEST-EVENT",
    title: "Test Event",
    category: "Test",
  };

  const result = EventSchema.parse(event);
  assertEquals(result.mutually_exclusive, false);
});

Deno.test("EventSchema defaults status to active", () => {
  const event = {
    event_ticker: "TEST-EVENT",
    title: "Test Event",
    category: "Test",
  };

  const result = EventSchema.parse(event);
  assertEquals(result.status, "active");
});

Deno.test("EventSchema rejects empty event_ticker", () => {
  const event = {
    event_ticker: "",
    title: "Test",
    category: "Test",
  };

  assertThrows(() => EventSchema.parse(event));
});

Deno.test("EventSchema allows null series_ticker", () => {
  const event = {
    event_ticker: "STANDALONE",
    title: "Standalone Event",
    category: "Test",
    series_ticker: null,
  };

  const result = EventSchema.parse(event);
  assertEquals(result.series_ticker, null);
});

Deno.test("fromKalshiEvent converts API response", () => {
  const kalshiEvent: KalshiEvent = {
    event_ticker: "KXBTC-24DEC31",
    title: "Will Bitcoin exceed $100k?",
    category: "Crypto",
    series_ticker: "KXBTC",
    strike_date: "2024-12-31",
    mutually_exclusive: true,
    sub_title: "Year-end prediction",
    event_type: "binary",
  };

  const event = fromKalshiEvent(kalshiEvent);

  assertEquals(event.event_ticker, "KXBTC-24DEC31");
  assertEquals(event.title, "Will Bitcoin exceed $100k?");
  assertEquals(event.status, "active");
  // Extra fields should be stripped
  assertEquals("sub_title" in event, false);
});

Deno.test("fromKalshiEvent maps status from API", () => {
  // Kalshi uses "open" for active events
  const openEvent = fromKalshiEvent({
    event_ticker: "A", title: "A", category: "C",
    series_ticker: null, strike_date: null, mutually_exclusive: false,
    status: "open",
  });
  assertEquals(openEvent.status, "active");

  // Closed events
  const closedEvent = fromKalshiEvent({
    event_ticker: "B", title: "B", category: "C",
    series_ticker: null, strike_date: null, mutually_exclusive: false,
    status: "closed",
  });
  assertEquals(closedEvent.status, "closed");

  // Settled events
  const settledEvent = fromKalshiEvent({
    event_ticker: "C", title: "C", category: "C",
    series_ticker: null, strike_date: null, mutually_exclusive: false,
    status: "settled",
  });
  assertEquals(settledEvent.status, "settled");

  // No status defaults to active
  const noStatusEvent = fromKalshiEvent({
    event_ticker: "D", title: "D", category: "C",
    series_ticker: null, strike_date: null, mutually_exclusive: false,
  });
  assertEquals(noStatusEvent.status, "active");
});
