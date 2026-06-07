/**
 * Pure window-resolution logic for ssmd hols Binance 1m OHLCV generation.
 *
 * Kept in a dependency-free module (no DuckDB/GCS/SMTP imports) so it can be
 * unit-tested under the standard `deno task test` permission set, which does
 * not grant --allow-ffi. hols.ts re-exports these symbols.
 */

export const DEFAULT_TRAILING_MINUTES = 600;

export type HolsGenerateMode = "daily" | "intraday";

export type Hols1mWindow = {
  dateStr: string;
  startMs: number;
  endMs: number;
  partial: boolean;
};

/**
 * Resolve the single-day fetch window for Binance 1m OHLCV generation.
 *
 * - daily:    yesterday's complete UTC day [00:00, next 00:00). partial=false.
 *             This is the batch behavior — byte-identical to the legacy
 *             single-day path used by the daily CronJob.
 * - intraday: TODAY, a trailing window ending at the current minute (floored
 *             to the minute boundary) and starting `trailingMinutes` earlier.
 *             partial=true. Used to keep a consumer's most recent ~N contiguous
 *             1-minute bars fresh between daily batch runs.
 *
 * The GCS path is keyed on `dateStr` for both modes, so an intraday run
 * overwrites today's partial file and the later daily run replaces it.
 */
export function resolveBinance1mWindow(
  now: Date,
  opts: { mode: HolsGenerateMode; trailingMinutes?: number },
): Hols1mWindow {
  const floorMin = (ms: number) => Math.floor(ms / 60_000) * 60_000;
  if (opts.mode === "intraday") {
    const endMs = floorMin(now.getTime());
    const startMs = endMs - (opts.trailingMinutes ?? DEFAULT_TRAILING_MINUTES) * 60_000;
    return {
      dateStr: new Date(endMs).toISOString().slice(0, 10),
      startMs,
      endMs,
      partial: true,
    };
  }
  const y = new Date(now);
  y.setUTCDate(y.getUTCDate() - 1);
  const dateStr = y.toISOString().slice(0, 10);
  const startMs = Date.parse(`${dateStr}T00:00:00Z`);
  return { dateStr, startMs, endMs: startMs + 86_400_000, partial: false };
}
