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

Deno.test("FEED_PATHS does not register the removed massive feed", () => {
  assertEquals(FEED_PATHS["massive"], undefined);
  assertEquals(VALID_DATA_FEEDS.includes("massive"), false);
  assertEquals(TRADE_CONFIG["massive"], undefined);
  assertEquals(PRICE_CONFIG["massive"], undefined);
  assertEquals(FEED_TYPES["massive"], undefined);
  assertEquals(VOLUME_UNITS["massive"], undefined);
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
