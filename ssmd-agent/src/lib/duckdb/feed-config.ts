/**
 * Feed path mappings and column configuration for DuckDB parquet queries.
 * Ported from ssmd-mcp Python config.py.
 */

/** GCS path prefix per feed (matches parquet-gen layout: prefix/prefix/stream) */
export const FEED_PATHS: Record<string, string> = {
  "kalshi": "kalshi/kalshi/crypto",
  "kraken-futures": "kraken-futures/kraken-futures/futures",
  "kraken-spot": "kraken-spot/kraken-spot/spot",
  "massive": "massive/massive/massive",
};

export const VALID_DATA_FEEDS = Object.keys(FEED_PATHS);

/** Trade query configuration per feed */
export interface TradeConfig {
  fileType: string;
  tickerCol: string;
  priceCol: string;
  qtyCol: string | null;
  priceDivisor: number;
}

export const TRADE_CONFIG: Record<string, TradeConfig> = {
  "kalshi": {
    fileType: "trade",
    tickerCol: "market_ticker",
    priceCol: "price",
    qtyCol: "count",
    priceDivisor: 100,
  },
  "kraken-futures": {
    fileType: "trade",
    tickerCol: "product_id",
    priceCol: "price",
    qtyCol: "qty",
    priceDivisor: 1,
  },
  "kraken-spot": {
    fileType: "trade",
    tickerCol: "symbol",
    priceCol: "price",
    qtyCol: "qty",
    priceDivisor: 1,
  },
  "massive": {
    // Massive has no raw trade feed in parquet; the 1m OHLCV bars are the
    // closest queryable surface. close is the bar close price, volume the
    // bar share volume; prices are already in dollars (no divisor).
    fileType: "ohlcv_1m",
    tickerCol: "symbol",
    priceCol: "close",
    qtyCol: "volume",
    priceDivisor: 1,
  },
};

/** Price/ticker snapshot file type per feed */
export const PRICE_CONFIG: Record<string, { fileType: string; orderCol: string }> = {
  "kalshi": { fileType: "ticker", orderCol: "ts" },
  "kraken-futures": { fileType: "ticker", orderCol: "_received_at" },
  "kraken-spot": { fileType: "ticker", orderCol: "_received_at" },
  "massive": { fileType: "ohlcv_1m", orderCol: "start_ts_ms" },
};

/** Parquet types available per feed */
export const FEED_TYPES: Record<string, string[]> = {
  "kalshi": ["trade", "ticker"],
  "kraken-futures": ["trade", "ticker"],
  "kraken-spot": ["trade", "ticker"],
  "massive": ["ohlcv_1s", "ohlcv_1m"],
};

/**
 * Build GCS parquet path using s3:// protocol (DuckDB httpfs).
 * Returns glob path if hour is null, specific file path otherwise.
 */
export function gcsParquetPath(
  bucket: string,
  feed: string,
  date: string,
  fileType: string,
  hour?: string,
): string {
  const prefix = FEED_PATHS[feed] ?? feed;
  if (hour) {
    return `s3://${bucket}/${prefix}/${date}/${fileType}_${hour}.parquet`;
  }
  return `s3://${bucket}/${prefix}/${date}/${fileType}_*.parquet`;
}
