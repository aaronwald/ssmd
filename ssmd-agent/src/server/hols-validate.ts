/**
 * Pure computation helpers for the /v1/hols/validate endpoint's Binance 1m
 * section.
 *
 * Kept in a dependency-free module (no DuckDB/GCS imports) so the gap/coverage/
 * boundary logic can be unit-tested under the standard `deno task test`
 * permission set, which does not grant the FFI access DuckDB needs. routes.ts
 * imports these symbols and feeds them rows pulled from DuckDB.
 */

/** Full UTC day of 1-minute bars. */
export const FULL_DAY_BARS = 1440;

/** Default trailing window for intraday partial-day files (mirrors hols-window). */
export const DEFAULT_PARTIAL_EXPECTED_BARS = 600;

/** Tolerance (in bars) applied to partial/full coverage short checks. */
export const DEFAULT_COVERAGE_TOLERANCE = 1;

/** One minute, in seconds — the 1m bar slot width. */
export const SLOT_SECONDS = 60;

/** A single bar row, reduced to the fields needed for gap analysis. */
export type TickerBarRow = {
  symbol: string;
  /** Bar timestamp as a unix epoch in SECONDS, on a minute boundary. */
  unix: number;
};

/** Per-symbol completeness: row count and number of missing 60s slots. */
export type TickerGapInfo = {
  bars: number;
  gaps: number;
};

/**
 * Compute per-symbol bar counts and interior gaps.
 *
 * For each symbol, `gaps` = expected_slots - present_slots over the symbol's own
 * [min, max] bar range, where expected_slots = (maxUnix - minUnix) / 60 + 1.
 * Gaps therefore only counts holes INTERIOR to the symbol's observed range; a
 * contiguous run reports 0 and a single missing minute reports 1. Duplicate
 * timestamps do not inflate the present-slot count.
 */
export function perTickerGaps(
  rows: ReadonlyArray<TickerBarRow>,
): Record<string, TickerGapInfo> {
  const bySymbol = new Map<
    string,
    { count: number; min: number; max: number; slots: Set<number> }
  >();

  for (const row of rows) {
    const unix = Number(row.unix);
    if (!Number.isFinite(unix)) continue;
    const existing = bySymbol.get(row.symbol);
    if (existing) {
      existing.count += 1;
      if (unix < existing.min) existing.min = unix;
      if (unix > existing.max) existing.max = unix;
      existing.slots.add(unix);
    } else {
      bySymbol.set(row.symbol, {
        count: 1,
        min: unix,
        max: unix,
        slots: new Set([unix]),
      });
    }
  }

  const out: Record<string, TickerGapInfo> = {};
  for (const [symbol, agg] of bySymbol) {
    const expectedSlots = Math.floor((agg.max - agg.min) / SLOT_SECONDS) + 1;
    const presentSlots = agg.slots.size;
    const gaps = Math.max(0, expectedSlots - presentSlots);
    out[symbol] = { bars: agg.count, gaps };
  }
  return out;
}

/** Pre-aggregated per-symbol row as returned by the binance-1m GROUP BY query. */
export type TickerAggregateRow = {
  symbol: string;
  bars: number; // total bar rows (incl. duplicate timestamps)
  presentSlots: number; // DISTINCT minute-slot timestamps
  minUnix: number;
  maxUnix: number;
};

/**
 * Per-symbol bar/gap info from pre-aggregated rows — the SQL-pushdown equivalent
 * of perTickerGaps. gaps = max(0, expectedSlots - presentSlots),
 * expectedSlots = floor((max-min)/SLOT_SECONDS)+1. Skips rows with a missing
 * symbol or non-finite bounds (degrades, never NaN).
 */
export function gapsFromAggregateRows(
  rows: ReadonlyArray<TickerAggregateRow>,
): Record<string, TickerGapInfo> {
  const out: Record<string, TickerGapInfo> = {};
  for (const r of rows) {
    if (!r || typeof r.symbol !== "string" || r.symbol.length === 0) continue;
    const min = Number(r.minUnix);
    const max = Number(r.maxUnix);
    const present = Number(r.presentSlots);
    if (!Number.isFinite(min) || !Number.isFinite(max) || !Number.isFinite(present)) {
      continue;
    }
    const expectedSlots = Math.floor((max - min) / SLOT_SECONDS) + 1;
    const bars = Number(r.bars);
    out[r.symbol] = {
      bars: Number.isFinite(bars) ? bars : 0,
      gaps: Math.max(0, expectedSlots - present),
    };
  }
  return out;
}

/** Result of a coverage short-check. */
export type CoverageDecision = {
  /** The bar count the worst-covered symbol was compared against. */
  expectedBars: number;
  /** True when the minimum per-ticker coverage falls below expected - tolerance. */
  short: boolean;
};

/**
 * Decide whether the worst-covered ticker is "short" for the file's mode.
 *
 * - partial=true:  compare against `expectedBars` (e.g. the 600-bar trailing
 *                  window) so a partial-day file is not flagged for lacking the
 *                  full 1440 bars.
 * - partial=false: compare against the full 1440-bar day (backward-compatible
 *                  default, matching the legacy 1440 coverage behavior).
 *
 * A small `tolerance` (bars) absorbs benign edge effects (e.g. a single
 * boundary minute). `short` is true only when minBarsPerTicker is more than
 * `tolerance` below `expectedBars`.
 */
export function decidePartialCoverage(opts: {
  minBarsPerTicker: number;
  partial: boolean;
  expectedBars?: number;
  tolerance?: number;
}): CoverageDecision {
  const tolerance = opts.tolerance ?? DEFAULT_COVERAGE_TOLERANCE;
  const expectedBars = opts.partial
    ? (opts.expectedBars ?? DEFAULT_PARTIAL_EXPECTED_BARS)
    : (opts.expectedBars ?? FULL_DAY_BARS);
  const short = opts.minBarsPerTicker < expectedBars - tolerance;
  return { expectedBars, short };
}

/** Result of a day-boundary contiguity check between two adjacent files. */
export type BoundaryGap = {
  /** True when todayFirst is exactly one slot (60s) after prevLast. */
  contiguous: boolean;
  /**
   * Seconds of MISSING coverage at the join: max(0, (todayFirst - prevLast) -
   * 60). 0 when contiguous. Undefined when the two timestamps overlap or are
   * out of order (todayFirst <= prevLast), which is reported as non-contiguous.
   */
  gapSeconds?: number;
};

/**
 * Compute day-boundary contiguity between the last bar of the previous day's
 * file (`prevLastUnix`, seconds) and the first bar of the requested date's file
 * (`todayFirstUnix`, seconds).
 *
 * Contiguous iff todayFirst == prevLast + 60. A larger forward delta reports
 * the missing seconds beyond the expected single slot. An overlap or reversed
 * order (todayFirst <= prevLast) is non-contiguous with no meaningful positive
 * gap.
 */
export function boundaryGapSeconds(
  prevLastUnix: number,
  todayFirstUnix: number,
): BoundaryGap {
  const delta = todayFirstUnix - prevLastUnix;
  if (delta <= 0) {
    return { contiguous: false };
  }
  const missing = delta - SLOT_SECONDS;
  return { contiguous: missing === 0, gapSeconds: Math.max(0, missing) };
}
