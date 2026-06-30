import { assertEquals, assertStringIncludes, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { binanceWsDailyGcsPath, buildBinanceAggregateSQL } from "../../src/cli/commands/hols-binance-agg.ts";

Deno.test("binanceWsDailyGcsPath uses the flat hols layout and the -binance-ws suffix", () => {
  assertEquals(
    binanceWsDailyGcsPath("2026-06-29"),
    "hols/crypto/daily/2026-06-29/ohlcv-1m-binance-ws.parquet",
  );
});

Deno.test("binanceWsDailyGcsPath rejects a malformed date", () => {
  assertThrows(() => binanceWsDailyGcsPath("2026-6-29"), Error, "invalid date");
});

Deno.test("buildBinanceAggregateSQL reads the glob, writes ZSTD parquet, tags binance", () => {
  const sql = buildBinanceAggregateSQL("/tmp/bt/*.parquet", "/tmp/out.parquet", "2026-06-29", "2026-06-29");
  assertStringIncludes(sql, "read_parquet('/tmp/bt/*.parquet')");
  assertStringIncludes(sql, "TO '/tmp/out.parquet'");
  assertStringIncludes(sql, "FORMAT PARQUET, COMPRESSION ZSTD");
  assertStringIncludes(sql, "'binance_spot_trades'::VARCHAR as source");
  assertStringIncludes(sql, "'ws'::VARCHAR as method");
  assertStringIncludes(sql, "'binance'::VARCHAR as exchange");
});

Deno.test("buildBinanceAggregateSQL buckets by minute from exchange_ts_ms millis", () => {
  const sql = buildBinanceAggregateSQL("/tmp/bt/*.parquet", "/tmp/out.parquet", "2026-06-29", "2026-06-29");
  // millis epoch -> TIMESTAMP via make_timestamp(micros); minute truncation.
  assertStringIncludes(sql, "make_timestamp(exchange_ts_ms * 1000)");
  assertStringIncludes(sql, "DATE_TRUNC('minute'");
  // binance symbol is already slashless -> hols_ticker = symbol (no REPLACE).
  assertStringIncludes(sql, "symbol::VARCHAR as hols_ticker");
});

Deno.test("buildBinanceAggregateSQL computes base+quote volume and the taker split", () => {
  const sql = buildBinanceAggregateSQL("/tmp/bt/*.parquet", "/tmp/out.parquet", "2026-06-29", "2026-06-29");
  assertStringIncludes(sql, "SUM(qty * price)::DOUBLE as volume");        // quote
  assertStringIncludes(sql, "SUM(qty)::DOUBLE as volume_from");           // base
  assertStringIncludes(sql, "COUNT(*)::BIGINT as tradecount");
  // taker buy = aggressor buy = NOT buyer-maker; taker sell = buyer-maker.
  assertStringIncludes(sql, "is_buyer_maker = false THEN qty * price");
  assertStringIncludes(sql, "is_buyer_maker = false THEN qty ");
  assertStringIncludes(sql, "is_buyer_maker = true THEN qty * price");
  assertStringIncludes(sql, "is_buyer_maker = true THEN qty ");
  assertStringIncludes(sql, "as taker_buy_volume");
  assertStringIncludes(sql, "as taker_buy_volume_from");
  assertStringIncludes(sql, "as taker_sell_volume");
  assertStringIncludes(sql, "as taker_sell_volume_from");
});

Deno.test("buildBinanceAggregateSQL zero-fills empty minutes and validates args", () => {
  const sql = buildBinanceAggregateSQL("/tmp/bt/*.parquet", "/tmp/out.parquet", "2026-06-29", "2026-06-29");
  assertStringIncludes(sql, "generate_series");
  assertStringIncludes(sql, "CROSS JOIN");
  assertStringIncludes(sql, "COALESCE(ta.volume, 0)");
  assertThrows(() => buildBinanceAggregateSQL("", "/tmp/o.parquet", "2026-06-29", "2026-06-29"), Error, "inputGlob is required");
  assertThrows(() => buildBinanceAggregateSQL("/tmp/bt/*.parquet", "", "2026-06-29", "2026-06-29"), Error, "parquetPath is required");
});
