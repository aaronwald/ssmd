import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  INTRADAY_INTERVAL,
  resolveBinance1mWindow,
  resolveBinanceInterval,
} from "../../src/cli/commands/hols-window.ts";

// Mirrors the GCS filename construction in runHolsGenerateBinance (hols.ts):
//   hols/crypto/daily/${dateStr}/ohlcv-${interval}m-binance.parquet
const binanceFilename = (interval: number) => `ohlcv-${interval}m-binance.parquet`;

Deno.test("intraday partial: targets TODAY with trailing window", () => {
  const now = new Date("2026-06-07T14:32:10Z");
  const w = resolveBinance1mWindow(now, { mode: "intraday", trailingMinutes: 600 });
  assertEquals(w.dateStr, "2026-06-07");
  assertEquals(w.startMs, Date.parse("2026-06-07T04:32:00Z")); // now - 600m, minute-floored
  assertEquals(w.endMs, Date.parse("2026-06-07T14:32:00Z")); // minute-floored now
  assertEquals(w.partial, true);
});

Deno.test("daily batch: unchanged — yesterday, full UTC day", () => {
  const now = new Date("2026-06-07T01:15:00Z");
  const w = resolveBinance1mWindow(now, { mode: "daily" });
  assertEquals(w.dateStr, "2026-06-06");
  assertEquals(w.startMs, Date.parse("2026-06-06T00:00:00Z"));
  assertEquals(w.endMs, Date.parse("2026-06-07T00:00:00Z"));
  assertEquals(w.partial, false);
});

Deno.test("intraday default trailingMinutes is 600", () => {
  const now = new Date("2026-06-07T14:32:10Z");
  const w = resolveBinance1mWindow(now, { mode: "intraday" });
  assertEquals(w.startMs, Date.parse("2026-06-07T04:32:00Z"));
  assertEquals(w.endMs, Date.parse("2026-06-07T14:32:00Z"));
  assertEquals(w.partial, true);
});

Deno.test("intraday floors seconds to the minute boundary", () => {
  const now = new Date("2026-06-07T14:32:59.999Z");
  const w = resolveBinance1mWindow(now, { mode: "intraday", trailingMinutes: 1 });
  assertEquals(w.endMs, Date.parse("2026-06-07T14:32:00Z"));
  assertEquals(w.startMs, Date.parse("2026-06-07T14:31:00Z"));
});

Deno.test("daily batch crosses month boundary correctly", () => {
  const now = new Date("2026-07-01T00:05:00Z");
  const w = resolveBinance1mWindow(now, { mode: "daily" });
  assertEquals(w.dateStr, "2026-06-30");
  assertEquals(w.startMs, Date.parse("2026-06-30T00:00:00Z"));
  assertEquals(w.endMs, Date.parse("2026-07-01T00:00:00Z"));
  assertEquals(w.partial, false);
});

Deno.test("intraday forces 1m interval when --interval is absent (defaulted to 5)", () => {
  // DEFAULT_INTERVAL (5) flows in when --interval is omitted; intraday must override.
  const r = resolveBinanceInterval("intraday", 5);
  assertEquals(r.interval, INTRADAY_INTERVAL);
  assertEquals(r.interval, 1);
  assertEquals(r.overridden, true);
  assertEquals(binanceFilename(r.interval), "ohlcv-1m-binance.parquet");
});

Deno.test("intraday forces 1m interval when --interval is wrong (e.g. 15)", () => {
  const r = resolveBinanceInterval("intraday", 15);
  assertEquals(r.interval, 1);
  assertEquals(r.overridden, true);
  assertEquals(binanceFilename(r.interval), "ohlcv-1m-binance.parquet");
});

Deno.test("intraday with --interval 1 is a no-op (not flagged as overridden)", () => {
  const r = resolveBinanceInterval("intraday", 1);
  assertEquals(r.interval, 1);
  assertEquals(r.overridden, false);
  assertEquals(binanceFilename(r.interval), "ohlcv-1m-binance.parquet");
});

Deno.test("daily interval is passed through unchanged (5m and 1m)", () => {
  const five = resolveBinanceInterval("daily", 5);
  assertEquals(five.interval, 5);
  assertEquals(five.overridden, false);
  assertEquals(binanceFilename(five.interval), "ohlcv-5m-binance.parquet");

  const one = resolveBinanceInterval("daily", 1);
  assertEquals(one.interval, 1);
  assertEquals(one.overridden, false);
  assertEquals(binanceFilename(one.interval), "ohlcv-1m-binance.parquet");
});
