/**
 * SQL query builders for DuckDB parquet queries.
 * Ported from ssmd-mcp Python tools.py.
 */
import { TRADE_CONFIG, PRICE_CONFIG, FEED_PATHS, gcsParquetPath } from "./feed-config.ts";

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
    ? `SUM(${tc.qtyCol}) as total_volume,`
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

/**
 * Expand $FEED_PATH(feed) and $FEED_PATH(feed, date) macros in SQL.
 */
export function expandFeedPath(
  sql: string,
  bucket: string,
  defaultFeed?: string,
  defaultDate?: string,
): string {
  const today = defaultDate ?? new Date().toISOString().slice(0, 10);

  return sql.replace(
    /\$FEED_PATH\(\s*([a-z-]+)\s*(?:,\s*([0-9-]+)\s*)?\)/g,
    (_match, feed: string, date?: string) => {
      const d = date ?? today;
      const prefix = FEED_PATHS[feed] ?? feed;
      return `s3://${bucket}/${prefix}/${d}/`;
    },
  );
}

/**
 * Validate SQL is SELECT-only (safety check for query_raw).
 */
export function validateSelectOnly(sql: string): boolean {
  const normalized = sql.trim().toUpperCase();
  const forbidden = ["INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE", "GRANT", "REVOKE"];
  // Check if the statement starts with a forbidden keyword
  for (const kw of forbidden) {
    if (normalized.startsWith(kw)) return false;
  }
  // Also check for these as standalone statements after semicolons
  const statements = normalized.split(";").map(s => s.trim()).filter(Boolean);
  for (const stmt of statements) {
    for (const kw of forbidden) {
      if (stmt.startsWith(kw)) return false;
    }
  }
  return true;
}

/**
 * Enforce LIMIT 1000 safety on raw queries.
 */
export function enforceLimitSafety(sql: string): string {
  if (/\bLIMIT\b/i.test(sql)) return sql;
  return sql.replace(/;?\s*$/, " LIMIT 1000");
}
