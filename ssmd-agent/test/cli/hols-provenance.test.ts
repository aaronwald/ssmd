import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { DuckDBInstance } from "@duckdb/node-api";
import { convertNdjsonToParquet } from "../../src/cli/commands/hols.ts";

Deno.test("convertNdjsonToParquet carries provenance columns", async () => {
  const dir = await Deno.makeTempDir();
  const ndjson = `${dir}/in.ndjson`;
  const parquet = `${dir}/out.parquet`;
  const row = {
    symbol: "BTC/USDT",
    hols_ticker: "BTCUSDT",
    source: "binance_spot",
    method: "rest",
    exchange: "binance",
    interval: "1m",
    date: "2026-06-26T00:00:00",
    date_close: "2026-06-26T00:01:00",
    unix: 1782518400,
    close_unix: 1782518460,
    open: 1,
    high: 2,
    low: 0.5,
    close: 1.5,
    volume: 10,
    volume_from: 15,
    tradecount: 3,
    marketorder_volume: 4,
    marketorder_volume_from: 6,
  };
  await Deno.writeTextFile(ndjson, JSON.stringify(row) + "\n");
  await convertNdjsonToParquet(ndjson, parquet);
  const inst = await DuckDBInstance.create();
  const conn = await inst.connect();
  const res = await conn.run(
    `SELECT method, exchange, interval FROM read_parquet('${parquet}')`,
  );
  const rows = await res.getRows();
  assertEquals(rows[0], ["rest", "binance", "1m"]);
});
