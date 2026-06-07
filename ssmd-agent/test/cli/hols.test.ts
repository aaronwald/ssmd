import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { resolveBinance1mWindow } from "../../src/cli/commands/hols-window.ts";

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
