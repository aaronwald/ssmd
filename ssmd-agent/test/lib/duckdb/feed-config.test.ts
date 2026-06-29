import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  FEED_PATHS,
  FEED_TYPES,
  gcsParquetPath,
  PRICE_CONFIG,
  TRADE_CONFIG,
  VALID_DATA_FEEDS,
} from "../../../src/lib/duckdb/feed-config.ts";
import { VOLUME_UNITS } from "../../../src/lib/duckdb/queries.ts";

Deno.test("FEED_PATHS registers massive with the archiver double-nested prefix", () => {
  assertEquals(FEED_PATHS["massive"], "massive/massive/massive");
});

Deno.test("VALID_DATA_FEEDS auto-includes massive via FEED_PATHS", () => {
  assertEquals(VALID_DATA_FEEDS.includes("massive"), true);
});

Deno.test("TRADE_CONFIG massive maps to 1m bars (symbol/close/volume, no divisor)", () => {
  const tc = TRADE_CONFIG["massive"];
  assertEquals(tc.fileType, "ohlcv_1m");
  assertEquals(tc.tickerCol, "symbol");
  assertEquals(tc.priceCol, "close");
  assertEquals(tc.qtyCol, "volume");
  assertEquals(tc.priceDivisor, 1);
});

Deno.test("PRICE_CONFIG massive orders by start_ts_ms on 1m bars", () => {
  assertEquals(PRICE_CONFIG["massive"], { fileType: "ohlcv_1m", orderCol: "start_ts_ms" });
});

Deno.test("FEED_TYPES massive exposes raw 1s and 1m bar types", () => {
  assertEquals(FEED_TYPES["massive"], ["ohlcv_1s", "ohlcv_1m"]);
});

Deno.test("VOLUME_UNITS massive is shares", () => {
  assertEquals(VOLUME_UNITS["massive"], "shares");
});

Deno.test("gcsParquetPath builds massive 1m glob from the nested prefix", () => {
  assertEquals(
    gcsParquetPath("ssmd-data", "massive", "2026-06-20", "ohlcv_1m"),
    "s3://ssmd-data/massive/massive/massive/2026-06-20/ohlcv_1m_*.parquet",
  );
});

Deno.test("FEED_PATHS registers binance with the archiver double-nested spot prefix", () => {
  assertEquals(FEED_PATHS["binance"], "binance/binance/spot");
});

Deno.test("VALID_DATA_FEEDS auto-includes binance via FEED_PATHS", () => {
  assertEquals(VALID_DATA_FEEDS.includes("binance"), true);
});

Deno.test("TRADE_CONFIG binance maps to trade parquet (symbol/price/qty, no divisor)", () => {
  const tc = TRADE_CONFIG["binance"];
  assertEquals(tc.fileType, "trade");
  assertEquals(tc.tickerCol, "symbol");
  assertEquals(tc.priceCol, "price");
  assertEquals(tc.qtyCol, "qty");
  assertEquals(tc.priceDivisor, 1);
});

Deno.test("PRICE_CONFIG binance reads the trade file ordered by exchange_ts_ms", () => {
  assertEquals(PRICE_CONFIG["binance"], { fileType: "trade", orderCol: "exchange_ts_ms" });
});

Deno.test("FEED_TYPES binance is trade-only (no ticker parquet)", () => {
  assertEquals(FEED_TYPES["binance"], ["trade"]);
});

Deno.test("VOLUME_UNITS binance is base_currency", () => {
  assertEquals(VOLUME_UNITS["binance"], "base_currency");
});

Deno.test("gcsParquetPath builds binance trade glob from the nested spot prefix", () => {
  assertEquals(
    gcsParquetPath("ssmd-data", "binance", "2026-06-29", "trade"),
    "s3://ssmd-data/binance/binance/spot/2026-06-29/trade_*.parquet",
  );
});
