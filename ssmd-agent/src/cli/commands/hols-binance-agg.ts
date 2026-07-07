/**
 * Pure (dependency-free) helpers for the binance WS daily 1m aggregate.
 *
 * Kept out of hols.ts so the GCS path builder and the aggregation SQL can be
 * unit-tested without the DuckDB native binding (--allow-ffi). Mirrors
 * hols-window.ts.
 *
 * Differs from the kraken aggregate (aggregateTradesToOhlcv in hols.ts):
 *   - timestamp column is `exchange_ts_ms` (Int64 epoch-millis), not a TIMESTAMP
 *     `timestamp` column → bucket via make_timestamp(exchange_ts_ms * 1000).
 *   - `symbol` is already slashless (BTCUSDT) → hols_ticker = symbol (no REPLACE).
 *   - taker split derives from `is_buyer_maker` (binance 1.1.0), not `ord_type`.
 *   - binance has no order-type data → marketorder_volume is NULL.
 *
 * SECURITY NOTE: inputGlob / parquetPath are trusted, internally-built /tmp
 * literals (never user input), and DuckDB COPY/read_parquet file paths must be
 * SQL literals (not bindable) — so they are interpolated exactly as the sibling
 * aggregate jobs do. Output existence / non-empty / row
 * count verification is the orchestrator's job (runHolsAggregateBinance in
 * hols.ts), which fails loud (Deno.exit(1)) on a zero-row result.
 */

/** Flat hols path. Distinct suffix from the REST-sourced ohlcv-1m-binance.parquet. */
export function binanceWsDailyGcsPath(endDateStr: string): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(endDateStr)) {
    throw new Error(`binanceWsDailyGcsPath: invalid date "${endDateStr}" (expected YYYY-MM-DD)`);
  }
  return `hols/crypto/daily/${endDateStr}/ohlcv-1m-binance-ws.parquet`;
}

/**
 * Build the DuckDB SQL that aggregates archived binance trade parquet
 * (columns: symbol, price, qty, exchange_ts_ms, is_buyer_maker, trade_id, ...)
 * into zero-filled 1-minute OHLCV bars over [startStr, endStr] inclusive.
 * Empty minutes get forward-/back-filled OHLC and zero volumes (mirrors the
 * kraken aggregate). Paths are trusted internally-built /tmp literals.
 */
export function buildBinanceAggregateSQL(
  inputGlob: string,
  parquetPath: string,
  startStr: string,
  endStr: string,
): string {
  if (!inputGlob) throw new Error("buildBinanceAggregateSQL: inputGlob is required");
  if (!parquetPath) throw new Error("buildBinanceAggregateSQL: parquetPath is required");
  if (!/^\d{4}-\d{2}-\d{2}$/.test(startStr) || !/^\d{4}-\d{2}-\d{2}$/.test(endStr)) {
    throw new Error("buildBinanceAggregateSQL: startStr/endStr must be YYYY-MM-DD");
  }
  if (startStr > endStr) {
    throw new Error(`buildBinanceAggregateSQL: startStr "${startStr}" must be <= endStr "${endStr}"`);
  }
  return `
    COPY (
      WITH trade_agg AS (
        SELECT
          symbol::VARCHAR as symbol,
          DATE_TRUNC('minute', make_timestamp(exchange_ts_ms * 1000)) as minute,
          arg_min(price, exchange_ts_ms)::DOUBLE as open,
          MAX(price)::DOUBLE as high,
          MIN(price)::DOUBLE as low,
          arg_max(price, exchange_ts_ms)::DOUBLE as close,
          SUM(qty * price)::DOUBLE as volume,
          SUM(qty)::DOUBLE as volume_from,
          COUNT(*)::BIGINT as tradecount,
          SUM(CASE WHEN is_buyer_maker = false THEN qty * price ELSE 0 END)::DOUBLE as taker_buy_volume,
          SUM(CASE WHEN is_buyer_maker = false THEN qty ELSE 0 END)::DOUBLE as taker_buy_volume_from,
          SUM(CASE WHEN is_buyer_maker = true THEN qty * price ELSE 0 END)::DOUBLE as taker_sell_volume,
          SUM(CASE WHEN is_buyer_maker = true THEN qty ELSE 0 END)::DOUBLE as taker_sell_volume_from
        FROM read_parquet('${inputGlob}')
        GROUP BY symbol, DATE_TRUNC('minute', make_timestamp(exchange_ts_ms * 1000))
      ),
      symbols AS (
        SELECT DISTINCT symbol::VARCHAR as symbol
        FROM read_parquet('${inputGlob}')
      ),
      minutes AS (
        SELECT generate_series::TIMESTAMP as minute
        FROM generate_series(
          TIMESTAMP '${startStr}',
          TIMESTAMP '${endStr}' + INTERVAL '1 day' - INTERVAL '1 minute',
          INTERVAL '1 minute'
        )
      ),
      spine AS (
        SELECT s.symbol, m.minute
        FROM symbols s CROSS JOIN minutes m
      ),
      with_fill AS (
        SELECT
          sp.symbol,
          sp.minute,
          ta.open, ta.high, ta.low, ta.close,
          COALESCE(ta.volume, 0)::DOUBLE as volume,
          COALESCE(ta.volume_from, 0)::DOUBLE as volume_from,
          COALESCE(ta.tradecount, 0)::BIGINT as tradecount,
          COALESCE(ta.taker_buy_volume, 0)::DOUBLE as taker_buy_volume,
          COALESCE(ta.taker_buy_volume_from, 0)::DOUBLE as taker_buy_volume_from,
          COALESCE(ta.taker_sell_volume, 0)::DOUBLE as taker_sell_volume,
          COALESCE(ta.taker_sell_volume_from, 0)::DOUBLE as taker_sell_volume_from,
          LAST_VALUE(ta.close IGNORE NULLS) OVER (
            PARTITION BY sp.symbol ORDER BY sp.minute
            ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
          ) as ffill_close,
          FIRST_VALUE(ta.open IGNORE NULLS) OVER (
            PARTITION BY sp.symbol ORDER BY sp.minute
            ROWS BETWEEN CURRENT ROW AND UNBOUNDED FOLLOWING
          ) as bfill_open
        FROM spine sp
        LEFT JOIN trade_agg ta ON sp.symbol = ta.symbol AND sp.minute = ta.minute
      )
      SELECT
        symbol::VARCHAR as symbol,
        symbol::VARCHAR as hols_ticker,
        'binance_spot_trades'::VARCHAR as source,
        'ws'::VARCHAR as method,
        'binance'::VARCHAR as exchange,
        '1m'::VARCHAR as interval,
        minute::TIMESTAMP as date,
        (minute + INTERVAL '1 minute')::TIMESTAMP as date_close,
        EPOCH(minute)::BIGINT as unix,
        EPOCH(minute + INTERVAL '1 minute')::BIGINT as close_unix,
        COALESCE(open, ffill_close, bfill_open)::DOUBLE as open,
        COALESCE(high, ffill_close, bfill_open)::DOUBLE as high,
        COALESCE(low, ffill_close, bfill_open)::DOUBLE as low,
        COALESCE(close, ffill_close, bfill_open)::DOUBLE as close,
        volume,
        volume_from,
        tradecount,
        taker_buy_volume,
        taker_buy_volume_from,
        taker_sell_volume,
        taker_sell_volume_from,
        NULL::DOUBLE as marketorder_volume,
        NULL::DOUBLE as marketorder_volume_from
      FROM with_fill
      ORDER BY symbol, minute
    ) TO '${parquetPath}' (FORMAT PARQUET, COMPRESSION ZSTD)
  `.trim();
}
