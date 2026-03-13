import { assertEquals } from "jsr:@std/assert";
import { dataCompleteness } from "./data-completeness.ts";
import type { CodeInput } from "./mod.ts";

function makeInput(
  freshnessBody: unknown,
  validateBody?: unknown,
  params?: Record<string, unknown>,
): CodeInput {
  const stages: Record<number, unknown> = {
    0: { output: JSON.stringify({ body: freshnessBody }) },
  };
  if (validateBody !== undefined) {
    stages[1] = { output: JSON.stringify({ body: validateBody }) };
  }
  return { stages, triggerInfo: {}, date: "2026-03-13", params };
}

Deno.test("data-completeness: all feeds fresh, no validation → skip", () => {
  const result = dataCompleteness(makeInput({
    feeds: [
      { feed: "kalshi", status: "fresh", newest_date: "2026-03-13", age_hours: 3, stale: false },
      { feed: "kraken-spot", status: "fresh", newest_date: "2026-03-13", age_hours: 4, stale: false },
    ],
  }));
  assertEquals(result.skip, true);
  assertEquals((result.result as Record<string, unknown>).allGood, true);
});

Deno.test("data-completeness: stale feed → no skip", () => {
  const result = dataCompleteness(makeInput({
    feeds: [
      { feed: "kalshi", status: "fresh", newest_date: "2026-03-13", age_hours: 3, stale: false },
      { feed: "kraken-spot", status: "stale", newest_date: "2026-03-12", age_hours: 20, stale: true },
    ],
  }));
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).allGood, false);
  const issues = (result.result as Record<string, unknown>).issues as string[];
  assertEquals(issues.length >= 1, true);
  assertEquals(issues.some((i: string) => i.includes("kraken-spot")), true);
});

Deno.test("data-completeness: validation low records → no skip", () => {
  const result = dataCompleteness(makeInput(
    {
      feeds: [
        { feed: "kalshi", status: "fresh", age_hours: 3, stale: false },
      ],
    },
    {
      kraken_rest: { total_records: 500, ticker_count: 17 },
      binance_5m: { total_records: 50000, ticker_count: 154 },
    },
    { minRecords: 1000 },
  ));
  assertEquals(result.skip, false);
  const issues = (result.result as Record<string, unknown>).issues as string[];
  assertEquals(issues.some((i: string) => i.includes("kraken_rest")), true);
});

Deno.test("data-completeness: validation zero tickers → issue", () => {
  const result = dataCompleteness(makeInput(
    {
      feeds: [
        { feed: "kalshi", status: "fresh", age_hours: 3, stale: false },
      ],
    },
    {
      kraken_rest: { total_records: 5000, ticker_count: 0 },
    },
  ));
  assertEquals(result.skip, false);
  const issues = (result.result as Record<string, unknown>).issues as string[];
  assertEquals(issues.some((i: string) => i.includes("zero tickers")), true);
});

Deno.test("data-completeness: no freshness output → issue", () => {
  const input: CodeInput = { stages: {}, triggerInfo: {}, date: "2026-03-13" };
  const result = dataCompleteness(input);
  assertEquals(result.skip, false);
  const issues = (result.result as Record<string, unknown>).issues as string[];
  assertEquals(issues[0], "No freshness stage output");
});
