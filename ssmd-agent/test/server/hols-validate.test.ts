import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  boundaryGapSeconds,
  decidePartialCoverage,
  perTickerGaps,
  type TickerBarRow,
} from "../../src/server/hols-validate.ts";

// ---------------------------------------------------------------------------
// perTickerGaps: count missing 60-second slots per symbol
// ---------------------------------------------------------------------------

Deno.test("perTickerGaps: contiguous run has gaps:0", () => {
  const base = 1_700_000_000; // arbitrary unix second on a minute boundary
  const rows: TickerBarRow[] = [
    { symbol: "BTCUSDT", unix: base },
    { symbol: "BTCUSDT", unix: base + 60 },
    { symbol: "BTCUSDT", unix: base + 120 },
  ];
  assertEquals(perTickerGaps(rows), {
    BTCUSDT: { bars: 3, gaps: 0 },
  });
});

Deno.test("perTickerGaps: a single 1-minute hole counts as gaps:1", () => {
  const base = 1_700_000_000;
  // base, base+60 missing, base+120 -> expected 3 slots, present 2 -> 1 gap
  const rows: TickerBarRow[] = [
    { symbol: "BTCUSDT", unix: base },
    { symbol: "BTCUSDT", unix: base + 120 },
  ];
  assertEquals(perTickerGaps(rows), {
    BTCUSDT: { bars: 2, gaps: 1 },
  });
});

Deno.test("perTickerGaps: multiple symbols computed independently", () => {
  const base = 1_700_000_000;
  const rows: TickerBarRow[] = [
    { symbol: "BTCUSDT", unix: base },
    { symbol: "BTCUSDT", unix: base + 60 },
    { symbol: "ETHUSDT", unix: base },
    { symbol: "ETHUSDT", unix: base + 180 }, // 2 holes (base+60, base+120)
  ];
  assertEquals(perTickerGaps(rows), {
    BTCUSDT: { bars: 2, gaps: 0 },
    ETHUSDT: { bars: 2, gaps: 2 },
  });
});

Deno.test("perTickerGaps: single bar has gaps:0", () => {
  const rows: TickerBarRow[] = [{ symbol: "BTCUSDT", unix: 1_700_000_000 }];
  assertEquals(perTickerGaps(rows), { BTCUSDT: { bars: 1, gaps: 0 } });
});

Deno.test("perTickerGaps: empty input is empty object", () => {
  assertEquals(perTickerGaps([]), {});
});

// ---------------------------------------------------------------------------
// decidePartialCoverage: partial-day vs full-day coverage decision
// ---------------------------------------------------------------------------

Deno.test("decidePartialCoverage: partial passes at expected_bars exactly", () => {
  const r = decidePartialCoverage({
    minBarsPerTicker: 600,
    partial: true,
    expectedBars: 600,
  });
  assertEquals(r.expectedBars, 600);
  assertEquals(r.short, false);
});

Deno.test("decidePartialCoverage: partial passes within tolerance", () => {
  const r = decidePartialCoverage({
    minBarsPerTicker: 598,
    partial: true,
    expectedBars: 600,
    tolerance: 5,
  });
  assertEquals(r.short, false);
});

Deno.test("decidePartialCoverage: partial fails when short beyond tolerance", () => {
  const r = decidePartialCoverage({
    minBarsPerTicker: 500,
    partial: true,
    expectedBars: 600,
    tolerance: 5,
  });
  assertEquals(r.expectedBars, 600);
  assertEquals(r.short, true);
});

Deno.test("decidePartialCoverage: full-day default compares against 1440", () => {
  const r = decidePartialCoverage({ minBarsPerTicker: 1440, partial: false });
  assertEquals(r.expectedBars, 1440);
  assertEquals(r.short, false);
});

Deno.test("decidePartialCoverage: full-day short flagged", () => {
  const r = decidePartialCoverage({ minBarsPerTicker: 1000, partial: false });
  assertEquals(r.expectedBars, 1440);
  assertEquals(r.short, true);
});

// ---------------------------------------------------------------------------
// boundaryGapSeconds: day-boundary contiguity from two timestamps
// ---------------------------------------------------------------------------

Deno.test("boundaryGapSeconds: contiguous when prevLast + 60 == todayFirst", () => {
  const prevLast = 1_700_000_000;
  const todayFirst = prevLast + 60;
  const r = boundaryGapSeconds(prevLast, todayFirst);
  assertEquals(r, { contiguous: true, gapSeconds: 0 });
});

Deno.test("boundaryGapSeconds: non-contiguous reports gap seconds", () => {
  const prevLast = 1_700_000_000;
  const todayFirst = prevLast + 180; // 2 missing minutes -> 120s gap beyond expected 60
  const r = boundaryGapSeconds(prevLast, todayFirst);
  assertEquals(r.contiguous, false);
  assertEquals(r.gapSeconds, 120);
});

Deno.test("boundaryGapSeconds: overlap (todayFirst <= prevLast) is non-contiguous", () => {
  const prevLast = 1_700_000_000;
  const r = boundaryGapSeconds(prevLast, prevLast);
  assertEquals(r.contiguous, false);
});
