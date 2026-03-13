import { assertEquals } from "jsr:@std/assert";
import { parquetQuality } from "./parquet-quality.ts";
import type { CodeInput } from "./mod.ts";

function makeInput(rows: unknown[], params?: Record<string, unknown>): CodeInput {
  return {
    stages: {
      0: { output: JSON.stringify(rows) },
    },
    triggerInfo: {},
    date: "2026-03-13",
    params,
  };
}

Deno.test("parquet-quality: all good → skip", () => {
  const result = parquetQuality(makeInput([
    { feed: "kalshi", column_name: "ticker", null_rate_pct: 0, duplicate_count: 0, record_count: 50000 },
    { feed: "kraken-spot", column_name: "price", null_rate_pct: 0.1, duplicate_count: 0, record_count: 30000 },
  ]));
  assertEquals(result.skip, true);
  assertEquals((result.result as Record<string, unknown>).allGood, true);
});

Deno.test("parquet-quality: high null rate → no skip", () => {
  const result = parquetQuality(makeInput([
    { feed: "kalshi", column_name: "ticker", null_rate_pct: 12.5, duplicate_count: 0, record_count: 50000 },
  ]));
  assertEquals(result.skip, false);
  const issues = (result.result as Record<string, unknown>).issues as string[];
  assertEquals(issues.length, 1);
  assertEquals(issues[0].includes("12.5% null"), true);
});

Deno.test("parquet-quality: duplicates → no skip", () => {
  const result = parquetQuality(makeInput([
    { feed: "kalshi", column_name: "ticker", null_rate_pct: 0, duplicate_count: 42, record_count: 50000 },
  ]));
  assertEquals(result.skip, false);
  const issues = (result.result as Record<string, unknown>).issues as string[];
  assertEquals(issues.some((i: string) => i.includes("42 duplicates")), true);
});

Deno.test("parquet-quality: custom thresholds", () => {
  const result = parquetQuality(makeInput(
    [{ feed: "kalshi", column_name: "price", null_rate_pct: 8, duplicate_count: 3, record_count: 1000 }],
    { maxNullRatePct: 10, maxDuplicates: 5 },
  ));
  assertEquals(result.skip, true);
  assertEquals((result.result as Record<string, unknown>).allGood, true);
});

Deno.test("parquet-quality: no stage output → error", () => {
  const input: CodeInput = { stages: {}, triggerInfo: {}, date: "2026-03-13" };
  const result = parquetQuality(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "No SQL stage output found");
});

Deno.test("parquet-quality: empty rows → error", () => {
  const result = parquetQuality(makeInput([]));
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "No parquet stats rows returned — data may be missing");
});
