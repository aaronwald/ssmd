import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { parseBacktestArgs, expandDateRange } from "../../src/cli/commands/backtest.ts";

Deno.test("parseBacktestArgs extracts signal name", () => {
  const args = parseBacktestArgs({
    _: ["backtest", "run", "spread-alert"],
  });

  assertEquals(args.signal, "spread-alert");
});

Deno.test("parseBacktestArgs extracts dates from comma-separated string", () => {
  const args = parseBacktestArgs({
    _: ["backtest", "run", "my-signal"],
    dates: "2025-12-25,2025-12-26,2025-12-27",
  });

  assertEquals(args.signal, "my-signal");
  assertEquals(args.dates, ["2025-12-25", "2025-12-26", "2025-12-27"]);
});

Deno.test("parseBacktestArgs expands from/to into dates", () => {
  const args = parseBacktestArgs({
    _: ["backtest", "run", "range-signal"],
    from: "2025-12-25",
    to: "2025-12-27",
  });

  assertEquals(args.signal, "range-signal");
  assertEquals(args.dates, ["2025-12-25", "2025-12-26", "2025-12-27"]);
});

Deno.test("parseBacktestArgs handles allow-dirty flag", () => {
  const args = parseBacktestArgs({
    _: ["backtest", "run", "test"],
    "allow-dirty": true,
  });

  assertEquals(args.allowDirty, true);
});

Deno.test("parseBacktestArgs handles no-wait flag", () => {
  const args = parseBacktestArgs({
    _: ["backtest", "run", "test"],
    "no-wait": true,
  });

  assertEquals(args.noWait, true);
});

Deno.test("parseBacktestArgs handles explicit sha", () => {
  const args = parseBacktestArgs({
    _: ["backtest", "run", "test"],
    sha: "abc1234",
  });

  assertEquals(args.sha, "abc1234");
});

Deno.test("expandDateRange generates correct date array", () => {
  const dates = expandDateRange("2025-12-25", "2025-12-27");

  assertEquals(dates.length, 3);
  assertEquals(dates, ["2025-12-25", "2025-12-26", "2025-12-27"]);
});

Deno.test("expandDateRange handles single date", () => {
  const dates = expandDateRange("2025-12-25", "2025-12-25");

  assertEquals(dates.length, 1);
  assertEquals(dates, ["2025-12-25"]);
});

Deno.test("expandDateRange handles month boundary", () => {
  const dates = expandDateRange("2025-01-30", "2025-02-02");

  assertEquals(dates.length, 4);
  assertEquals(dates, ["2025-01-30", "2025-01-31", "2025-02-01", "2025-02-02"]);
});
