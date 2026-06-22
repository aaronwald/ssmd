/**
 * Pure (dependency-free) helpers for the massive daily OHLCV generator.
 *
 * Kept out of hols.ts so the GCS path builder and the aggregation SQL can be
 * unit-tested without pulling in the DuckDB native binding (which requires
 * --allow-ffi). Mirrors the hols-window.ts split.
 */

/**
 * Flat GCS path for the daily massive aggregate. MUST match the FEED_CONFIG
 * ohlcv_1d flat layout (massive/equities/daily/{date}/ohlcv-1d-massive.parquet)
 * so the /v1/data/download endpoint resolves the same object this job writes.
 */
export function massiveDailyGcsPath(endDateStr: string): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(endDateStr)) {
    throw new Error(`massiveDailyGcsPath: invalid date "${endDateStr}" (expected YYYY-MM-DD)`);
  }
  return `massive/equities/daily/${endDateStr}/ohlcv-1d-massive.parquet`;
}

/**
 * Build the DuckDB SQL that aggregates massive 1m bars into ONE daily OHLCV row
 * per (symbol, date). Reads from `inputGlob` (a trusted, internally-built /tmp
 * glob), writes ZSTD parquet to `parquetPath`. Inputs are not user-supplied;
 * DuckDB COPY/read_parquet file paths must be SQL literals (not bindable), so
 * the paths are interpolated exactly as the sibling aggregate jobs do.
 *
 * IMPORTANT — Polygon's delayed feed emits multiple CUMULATIVE `AM` snapshots
 * for the same minute (e.g. an intermediate bar then the final bar with the full
 * minute's volume), so the raw archive holds several rows per (symbol, minute).
 * Summing them directly double-counts volume (verified ~+42% on AAPL) and makes
 * close ambiguous. We first collapse to the FINAL bar per (symbol, start_ts_ms)
 * — the snapshot with the largest cumulative volume (tie-break latest end_ts_ms)
 * — which matches Polygon's REST 1m bar exactly, then aggregate to daily.
 */
export function buildMassiveDailySQL(inputGlob: string, parquetPath: string): string {
  if (!inputGlob) throw new Error("buildMassiveDailySQL: inputGlob is required");
  if (!parquetPath) throw new Error("buildMassiveDailySQL: parquetPath is required");
  return `
    COPY (
      WITH final_bars AS (
        SELECT *
        FROM read_parquet('${inputGlob}')
        QUALIFY ROW_NUMBER() OVER (
          PARTITION BY symbol, start_ts_ms
          ORDER BY volume DESC, end_ts_ms DESC
        ) = 1
      )
      SELECT
        symbol::VARCHAR as symbol,
        DATE_TRUNC('day', to_timestamp(start_ts_ms / 1000.0))::DATE as date,
        arg_min(open, start_ts_ms)::DOUBLE as open,
        MAX(high)::DOUBLE as high,
        MIN(low)::DOUBLE as low,
        arg_max(close, end_ts_ms)::DOUBLE as close,
        SUM(volume)::DOUBLE as volume,
        (SUM(volume * COALESCE(vwap, (open + close) / 2.0))
          / NULLIF(SUM(volume), 0))::DOUBLE as vwap,
        COUNT(*)::BIGINT as bar_count,
        MIN(start_ts_ms)::BIGINT as first_bar_ts_ms,
        MAX(end_ts_ms)::BIGINT as last_bar_ts_ms
      FROM final_bars
      GROUP BY symbol, DATE_TRUNC('day', to_timestamp(start_ts_ms / 1000.0))
      ORDER BY symbol, date
    ) TO '${parquetPath}' (FORMAT PARQUET, COMPRESSION ZSTD)
  `.trim();
}
