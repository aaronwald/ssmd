/**
 * SQL query builders for DuckDB parquet queries.
 * Ported from ssmd-mcp Python tools.py.
 */
import { TRADE_CONFIG, PRICE_CONFIG, gcsParquetPath } from "./feed-config.ts";

/**
 * Build trade aggregation SQL for a feed.
 */
export function buildTradeSQL(
  bucket: string,
  feed: string,
  date: string,
  limit: number,
): string {
  const tc = TRADE_CONFIG[feed];
  if (!tc) throw new Error(`Unknown feed: ${feed}`);

  const path = gcsParquetPath(bucket, feed, date, tc.fileType);
  const priceExpr = tc.priceDivisor !== 1
    ? `${tc.priceCol} / ${tc.priceDivisor}.0`
    : tc.priceCol;
  const volumeExpr = tc.qtyCol
    ? `COALESCE(SUM(${tc.qtyCol}), 0) as total_volume,`
    : "";

  return `
    SELECT
      ${tc.tickerCol} as ticker,
      COUNT(*) as trade_count,
      ${volumeExpr}
      MIN(${priceExpr}) as min_price,
      MAX(${priceExpr}) as max_price,
      AVG(${priceExpr}) as avg_price
    FROM read_parquet('${path}')
    GROUP BY ${tc.tickerCol}
    ORDER BY trade_count DESC
    LIMIT ${limit}
  `.trim();
}

/** Volume unit per feed. */
export const VOLUME_UNITS: Record<string, string> = {
  "kalshi": "contracts",
  "kraken-futures": "base_currency",
  "polymarket": "usd",
};

/**
 * Build event-level volume aggregation SQL for a feed.
 * Groups trades by event identifier, returning trade counts and volume per event.
 */
export function buildEventVolumeSQL(
  bucket: string,
  feed: string,
  date: string,
  limit: number,
): string {
  if (feed === "kalshi") {
    const path = gcsParquetPath(bucket, feed, date, "trade");
    return `
      SELECT
        array_to_string(string_split(market_ticker, '-')[1:2], '-') AS event_id,
        COUNT(*) as total_trade_count,
        COALESCE(SUM(count), 0) as total_volume,
        COUNT(DISTINCT market_ticker) as market_count
      FROM read_parquet('${path}')
      GROUP BY 1
      ORDER BY total_volume DESC
      LIMIT ${limit}
    `.trim();
  }

  if (feed === "polymarket") {
    const path = gcsParquetPath(bucket, feed, date, "last_trade_price");
    return `
      SELECT
        market AS event_id,
        COUNT(*) as total_trade_count,
        SUM(size) as total_volume,
        COUNT(DISTINCT asset_id) as market_count
      FROM read_parquet('${path}')
      GROUP BY 1
      ORDER BY total_volume DESC
      LIMIT ${limit}
    `.trim();
  }

  if (feed === "kraken-futures") {
    const path = gcsParquetPath(bucket, feed, date, "trade");
    return `
      SELECT
        product_id AS event_id,
        COUNT(*) as total_trade_count,
        COALESCE(SUM(qty), 0) as total_volume
      FROM read_parquet('${path}')
      GROUP BY 1
      ORDER BY total_volume DESC
      LIMIT ${limit}
    `.trim();
  }

  throw new Error(`Unknown feed: ${feed}`);
}

/**
 * Build per-market volume SQL for a given event on a feed.
 * Returns top markets within a single event by volume.
 */
export function buildEventMarketsSQL(
  bucket: string,
  feed: string,
  date: string,
  eventIds: string[],
  topN: number,
): string {
  const escaped = eventIds.map((id) => `'${id.replace(/'/g, "''")}'`).join(",");

  if (feed === "kalshi") {
    const path = gcsParquetPath(bucket, feed, date, "trade");
    return `
      SELECT
        array_to_string(string_split(market_ticker, '-')[1:2], '-') AS event_id,
        market_ticker as ticker,
        COUNT(*) as trade_count,
        COALESCE(SUM(count), 0) as volume
      FROM read_parquet('${path}')
      WHERE array_to_string(string_split(market_ticker, '-')[1:2], '-') IN (${escaped})
      GROUP BY 1, 2
      QUALIFY ROW_NUMBER() OVER (PARTITION BY event_id ORDER BY volume DESC) <= ${topN}
      ORDER BY event_id, volume DESC
    `.trim();
  }

  if (feed === "polymarket") {
    const path = gcsParquetPath(bucket, feed, date, "last_trade_price");
    return `
      SELECT
        market AS event_id,
        asset_id as ticker,
        COUNT(*) as trade_count,
        SUM(size) as volume
      FROM read_parquet('${path}')
      WHERE market IN (${escaped})
      GROUP BY 1, 2
      QUALIFY ROW_NUMBER() OVER (PARTITION BY event_id ORDER BY volume DESC) <= ${topN}
      ORDER BY event_id, trade_count DESC
    `.trim();
  }

  if (feed === "kraken-futures") {
    const path = gcsParquetPath(bucket, feed, date, "trade");
    // For Kraken, event_id = product_id, so there's one market per "event"
    return `
      SELECT
        product_id AS event_id,
        product_id as ticker,
        COUNT(*) as trade_count,
        COALESCE(SUM(qty), 0) as volume
      FROM read_parquet('${path}')
      WHERE product_id IN (${escaped})
      GROUP BY 1, 2
      ORDER BY volume DESC
    `.trim();
  }

  throw new Error(`Unknown feed: ${feed}`);
}

/**
 * Build total volume summary SQL for a feed (all tickers aggregated).
 */
export function buildTotalVolumeSQL(
  bucket: string,
  feed: string,
  date: string,
): string {
  const tc = TRADE_CONFIG[feed];
  if (!tc) throw new Error(`Unknown feed: ${feed}`);

  const path = gcsParquetPath(bucket, feed, date, tc.fileType);
  const volumeExpr = tc.qtyCol
    ? `COALESCE(SUM(${tc.qtyCol}), 0)`
    : "0";

  return `
    SELECT
      COUNT(*) as total_trade_count,
      ${volumeExpr} as total_volume,
      COUNT(DISTINCT ${tc.tickerCol}) as active_tickers
    FROM read_parquet('${path}')
  `.trim();
}

/**
 * Build top tickers by volume SQL for a feed.
 */
export function buildTopTickersSQL(
  bucket: string,
  feed: string,
  date: string,
  limit: number,
): string {
  const tc = TRADE_CONFIG[feed];
  if (!tc) throw new Error(`Unknown feed: ${feed}`);

  const path = gcsParquetPath(bucket, feed, date, tc.fileType);
  const volumeExpr = tc.qtyCol
    ? `COALESCE(SUM(${tc.qtyCol}), 0)`
    : "COUNT(*)";

  return `
    SELECT
      ${tc.tickerCol} as ticker,
      COUNT(*) as trade_count,
      ${volumeExpr} as volume
    FROM read_parquet('${path}')
    GROUP BY ${tc.tickerCol}
    ORDER BY volume DESC
    LIMIT ${limit}
  `.trim();
}

/**
 * Build price snapshot SQL for a feed.
 */
export function buildPriceSQL(
  bucket: string,
  feed: string,
  date: string,
  hour?: string,
): string {
  const pc = PRICE_CONFIG[feed];
  if (!pc) throw new Error(`Unknown feed: ${feed}`);

  const path = gcsParquetPath(bucket, feed, date, pc.fileType, hour);

  if (feed === "kalshi") {
    return `
      SELECT
        market_ticker as ticker,
        yes_bid / 100.0 as yes_bid,
        yes_ask / 100.0 as yes_ask,
        no_bid / 100.0 as no_bid,
        no_ask / 100.0 as no_ask,
        last_price / 100.0 as last_price,
        volume,
        open_interest,
        ts
      FROM read_parquet('${path}')
      QUALIFY ROW_NUMBER() OVER (PARTITION BY market_ticker ORDER BY ${pc.orderCol} DESC) = 1
      ORDER BY volume DESC
    `.trim();
  }

  if (feed === "kraken-futures") {
    return `
      SELECT
        product_id as ticker,
        bid,
        ask,
        last,
        volume,
        funding_rate,
        mark_price
      FROM read_parquet('${path}')
      QUALIFY ROW_NUMBER() OVER (PARTITION BY product_id ORDER BY ${pc.orderCol} DESC) = 1
      ORDER BY volume DESC
    `.trim();
  }

  // polymarket
  return `
    SELECT
      market,
      asset_id,
      best_bid,
      best_ask,
      spread
    FROM read_parquet('${path}')
    QUALIFY ROW_NUMBER() OVER (PARTITION BY asset_id ORDER BY ${pc.orderCol} DESC) = 1
    ORDER BY spread ASC
  `.trim();
}

