import { assertEquals, assertStringIncludes, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { buildMassiveDailySQL, massiveDailyGcsPath } from "../../src/cli/commands/hols-massive.ts";

Deno.test("massiveDailyGcsPath uses the flat daily layout", () => {
  // Must match FEED_CONFIG ohlcv_1d flat resolution
  // (massive/equities/daily/{date}/ohlcv-1d-massive.parquet) so the download
  // endpoint and the generator agree on the object key.
  assertEquals(
    massiveDailyGcsPath("2026-06-20"),
    "massive/equities/daily/2026-06-20/ohlcv-1d-massive.parquet",
  );
});

Deno.test("massiveDailyGcsPath rejects a malformed date", () => {
  assertThrows(() => massiveDailyGcsPath("2026-6-20"), Error, "invalid date");
  assertThrows(() => massiveDailyGcsPath("not-a-date"), Error, "invalid date");
});

Deno.test("buildMassiveDailySQL reads the input glob and writes ZSTD parquet", () => {
  const sql = buildMassiveDailySQL("/tmp/bars/*.parquet", "/tmp/out.parquet");
  assertStringIncludes(sql, "read_parquet('/tmp/bars/*.parquet')");
  assertStringIncludes(sql, "TO '/tmp/out.parquet'");
  assertStringIncludes(sql, "FORMAT PARQUET, COMPRESSION ZSTD");
});

Deno.test("buildMassiveDailySQL aggregates to one daily row per (symbol, date)", () => {
  const sql = buildMassiveDailySQL("/tmp/bars/*.parquet", "/tmp/out.parquet");
  // open/close use first/last bar by timestamp; high/low are extremes.
  assertStringIncludes(sql, "arg_min(open, start_ts_ms)");
  assertStringIncludes(sql, "arg_max(close, end_ts_ms)");
  assertStringIncludes(sql, "MAX(high)");
  assertStringIncludes(sql, "MIN(low)");
  assertStringIncludes(sql, "SUM(volume)");
  assertStringIncludes(sql, "COUNT(*)::BIGINT as bar_count");
  assertStringIncludes(sql, "MIN(start_ts_ms)::BIGINT as first_bar_ts_ms");
  assertStringIncludes(sql, "MAX(end_ts_ms)::BIGINT as last_bar_ts_ms");
  // Grouped by symbol + truncated day; VWAP is volume-weighted with a fallback.
  assertStringIncludes(sql, "GROUP BY symbol, DATE_TRUNC('day', to_timestamp(start_ts_ms / 1000.0))");
  assertStringIncludes(sql, "NULLIF(SUM(volume), 0)");
});

Deno.test("buildMassiveDailySQL dedupes to the final bar per minute before summing", () => {
  // Polygon emits multiple cumulative AM snapshots per minute; summing them
  // double-counts volume (~+42% observed). The SQL must collapse to the final
  // (largest-volume, latest end_ts_ms) bar per (symbol, start_ts_ms) first.
  const sql = buildMassiveDailySQL("/tmp/bars/*.parquet", "/tmp/out.parquet");
  assertStringIncludes(sql, "PARTITION BY symbol, start_ts_ms");
  assertStringIncludes(sql, "ORDER BY volume DESC, end_ts_ms DESC");
  assertStringIncludes(sql, "= 1");
  // Aggregation reads from the deduped CTE, not the raw parquet.
  assertStringIncludes(sql, "FROM final_bars");
});

Deno.test("buildMassiveDailySQL validates required arguments", () => {
  assertThrows(() => buildMassiveDailySQL("", "/tmp/out.parquet"), Error, "inputGlob is required");
  assertThrows(() => buildMassiveDailySQL("/tmp/bars/*.parquet", ""), Error, "parquetPath is required");
});
