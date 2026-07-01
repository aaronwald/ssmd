import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  boundaryGapSeconds,
  decidePartialCoverage,
  gapsFromAggregateRows,
  perTickerGaps,
  type TickerAggregateRow,
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
// gapsFromAggregateRows: SQL-pushdown equivalent of perTickerGaps
// ---------------------------------------------------------------------------

/**
 * Reference in-memory aggregation mirroring the binance-1m GROUP BY query:
 * collapse raw (symbol, unix) rows into one pre-aggregated row per symbol.
 */
function aggregate(rows: TickerBarRow[]): TickerAggregateRow[] {
  const by = new Map<
    string,
    { bars: number; slots: Set<number>; min: number; max: number }
  >();
  for (const r of rows) {
    const e = by.get(r.symbol);
    if (e) {
      e.bars++;
      e.slots.add(r.unix);
      e.min = Math.min(e.min, r.unix);
      e.max = Math.max(e.max, r.unix);
    } else {
      by.set(r.symbol, {
        bars: 1,
        slots: new Set([r.unix]),
        min: r.unix,
        max: r.unix,
      });
    }
  }
  return [...by].map(([symbol, a]) => ({
    symbol,
    bars: a.bars,
    presentSlots: a.slots.size,
    minUnix: a.min,
    maxUnix: a.max,
  }));
}

Deno.test("gapsFromAggregateRows: contiguous run matches perTickerGaps (0 gaps)", () => {
  const base = 1_700_000_000;
  const rows: TickerBarRow[] = [
    { symbol: "BTCUSDT", unix: base },
    { symbol: "BTCUSDT", unix: base + 60 },
    { symbol: "BTCUSDT", unix: base + 120 },
  ];
  assertEquals(gapsFromAggregateRows(aggregate(rows)), perTickerGaps(rows));
  assertEquals(gapsFromAggregateRows(aggregate(rows)), {
    BTCUSDT: { bars: 3, gaps: 0 },
  });
});

Deno.test("gapsFromAggregateRows: one interior gap matches perTickerGaps", () => {
  const base = 1_700_000_000;
  // ETH: base+60 and base+180 -> expected 3 slots, present 2 -> 1 gap
  const rows: TickerBarRow[] = [
    { symbol: "ETHUSDT", unix: base + 60 },
    { symbol: "ETHUSDT", unix: base + 180 },
  ];
  assertEquals(gapsFromAggregateRows(aggregate(rows)), perTickerGaps(rows));
  assertEquals(gapsFromAggregateRows(aggregate(rows)), {
    ETHUSDT: { bars: 2, gaps: 1 },
  });
});

Deno.test("gapsFromAggregateRows: duplicate timestamp counts a bar but no gap", () => {
  const base = 1_700_000_000;
  // SOL: base+60 twice -> bars 2, distinct slots 1, range 0 -> 0 gaps
  const rows: TickerBarRow[] = [
    { symbol: "SOLUSDT", unix: base + 60 },
    { symbol: "SOLUSDT", unix: base + 60 },
  ];
  assertEquals(gapsFromAggregateRows(aggregate(rows)), perTickerGaps(rows));
  assertEquals(gapsFromAggregateRows(aggregate(rows)), {
    SOLUSDT: { bars: 2, gaps: 0 },
  });
});

Deno.test("gapsFromAggregateRows: multiple symbols match perTickerGaps", () => {
  const base = 1_700_000_000;
  const rows: TickerBarRow[] = [
    { symbol: "BTCUSDT", unix: base },
    { symbol: "BTCUSDT", unix: base + 60 },
    { symbol: "ETHUSDT", unix: base },
    { symbol: "ETHUSDT", unix: base + 180 },
    { symbol: "SOLUSDT", unix: base + 60 },
    { symbol: "SOLUSDT", unix: base + 60 },
  ];
  assertEquals(gapsFromAggregateRows(aggregate(rows)), perTickerGaps(rows));
});

Deno.test("gapsFromAggregateRows: empty input is empty object", () => {
  assertEquals(gapsFromAggregateRows([]), {});
  assertEquals(gapsFromAggregateRows(aggregate([])), perTickerGaps([]));
});

Deno.test("gapsFromAggregateRows: row with empty symbol is skipped", () => {
  const rows: TickerAggregateRow[] = [
    { symbol: "", bars: 5, presentSlots: 5, minUnix: 1, maxUnix: 300 },
    {
      symbol: "BTCUSDT",
      bars: 2,
      presentSlots: 2,
      minUnix: 1_700_000_000,
      maxUnix: 1_700_000_060,
    },
  ];
  assertEquals(gapsFromAggregateRows(rows), {
    BTCUSDT: { bars: 2, gaps: 0 },
  });
});

Deno.test("gapsFromAggregateRows: non-finite bounds skipped (never NaN)", () => {
  const rows: TickerAggregateRow[] = [
    {
      symbol: "BADUSDT",
      bars: 3,
      presentSlots: Number.NaN,
      minUnix: 1,
      maxUnix: 100,
    },
  ];
  assertEquals(gapsFromAggregateRows(rows), {});
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
