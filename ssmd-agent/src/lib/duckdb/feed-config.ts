/**
 * Feed path mappings and column configuration for DuckDB parquet queries.
 * Ported from ssmd-mcp Python config.py.
 */

/** GCS path prefix per feed (matches parquet-gen layout: prefix/prefix/stream) */
export const FEED_PATHS: Record<string, string> = {
  "kalshi": "kalshi/kalshi/crypto",
  "kraken-spot": "kraken-spot/kraken-spot/spot",
  "binance": "binance/binance/spot",
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
  "kraken-spot": {
    fileType: "trade",
    tickerCol: "symbol",
    priceCol: "price",
    qtyCol: "qty",
    priceDivisor: 1,
  },
  "binance": {
    // Binance spot is trade-only (no ticker parquet). price is already in
    // dollars (no divisor); qty is the base-asset trade quantity.
    fileType: "trade",
    tickerCol: "symbol",
    priceCol: "price",
    qtyCol: "qty",
    priceDivisor: 1,
  },
};

/** Price/ticker snapshot file type per feed */
export const PRICE_CONFIG: Record<string, { fileType: string; orderCol: string }> = {
  "kalshi": { fileType: "ticker", orderCol: "ts" },
  "kraken-spot": { fileType: "ticker", orderCol: "_received_at" },
  // Binance has no ticker parquet; the latest trade is the price snapshot.
  "binance": { fileType: "trade", orderCol: "exchange_ts_ms" },
};

/** Parquet types available per feed */
export const FEED_TYPES: Record<string, string[]> = {
  "kalshi": ["trade", "ticker"],
  "kraken-spot": ["trade", "ticker"],
  "binance": ["trade"],
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
