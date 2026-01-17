// ssmd-notifier/test/format.test.ts
import { assertEquals, assertStringIncludes } from "@std/assert";
import { formatPayload } from "../src/format.ts";

Deno.test("formatPayload - camelCase to Title Case", () => {
  const result = formatPayload({ dollarVolume: 100 });
  assertStringIncludes(result, "Dollar Volume:");
});

Deno.test("formatPayload - currency fields get $ prefix", () => {
  const result = formatPayload({ dollarVolume: 1234567.89 });
  assertStringIncludes(result, "$1,234,567.89");
});

Deno.test("formatPayload - price fields get $ prefix", () => {
  const result = formatPayload({ lastPrice: 0.55 });
  assertStringIncludes(result, "$0.55");
});

Deno.test("formatPayload - duration fields converted from ms", () => {
  const result = formatPayload({ windowMs: 1800000 });
  assertStringIncludes(result, "30m");
});

Deno.test("formatPayload - duration hours", () => {
  const result = formatPayload({ intervalMs: 3600000 });
  assertStringIncludes(result, "1h");
});

Deno.test("formatPayload - duration seconds", () => {
  const result = formatPayload({ delayMs: 5000 });
  assertStringIncludes(result, "5s");
});

Deno.test("formatPayload - percentage fields", () => {
  const result = formatPayload({ buyRatio: 0.65 });
  assertStringIncludes(result, "65%");
});

Deno.test("formatPayload - percentage with percent in name", () => {
  const result = formatPayload({ winPercent: 0.832 });
  assertStringIncludes(result, "83.2%");
});

Deno.test("formatPayload - ISO dates formatted", () => {
  const result = formatPayload({ lastUpdate: "2026-01-16T21:30:00.000Z" });
  assertStringIncludes(result, "2026-01-16 21:30 UTC");
});

Deno.test("formatPayload - numbers with thousand separators", () => {
  const result = formatPayload({ tradeCount: 12345 });
  assertStringIncludes(result, "12,345");
});

Deno.test("formatPayload - integers no decimals", () => {
  const result = formatPayload({ count: 100 });
  assertStringIncludes(result, "100");
  // Should not have ".00"
  assertEquals(result.includes(".00"), false);
});

Deno.test("formatPayload - booleans", () => {
  const result = formatPayload({ isActive: true, isClosed: false });
  assertStringIncludes(result, "Yes");
  assertStringIncludes(result, "No");
});

Deno.test("formatPayload - arrays joined", () => {
  const result = formatPayload({ tags: ["sports", "nba", "live"] });
  assertStringIncludes(result, "sports, nba, live");
});

Deno.test("formatPayload - null/undefined values", () => {
  const result = formatPayload({ value: null });
  assertStringIncludes(result, "-");
});

Deno.test("formatPayload - empty payload", () => {
  const result = formatPayload({});
  assertEquals(result, "");
});

Deno.test("formatPayload - null payload", () => {
  const result = formatPayload(null);
  assertEquals(result, "");
});

Deno.test("formatPayload - full volume signal payload", () => {
  const payload = {
    ticker: "INXD-26MAR28-NGMI",
    dollarVolume: 1234567.89,
    contractVolume: 500,
    tradeCount: 123,
    buyRatio: 0.65,
    windowMs: 1800000,
    lastUpdate: "2026-01-16T21:30:00.000Z",
  };

  const result = formatPayload(payload);

  assertStringIncludes(result, "Ticker: INXD-26MAR28-NGMI");
  assertStringIncludes(result, "Dollar Volume: $1,234,567.89");
  assertStringIncludes(result, "Contract Volume: 500");
  assertStringIncludes(result, "Trade Count: 123");
  assertStringIncludes(result, "Buy Ratio: 65%");
  assertStringIncludes(result, "Window Ms: 30m");
  assertStringIncludes(result, "Last Update: 2026-01-16 21:30 UTC");
});
