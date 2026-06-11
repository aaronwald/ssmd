/**
 * GCS signed URL generation for parquet data sharing.
 * Uses Workload Identity (signBlob) for V4 signed URLs.
 */
import { Storage } from "@google-cloud/storage";

/** Feed → GCS path mapping (matches parquet-gen CronJob layout) */
export const FEED_CONFIG: Record<string, FeedInfo> = {
  "kalshi": {
    prefix: "kalshi",
    stream: "crypto",
    messageTypes: ["ticker", "trade", "market_lifecycle_v2"],
    description: "Kalshi crypto markets — ticker & trade events, incl. the 15-minute KX…15M contracts.",
  },
  "kraken-futures": {
    prefix: "kraken-futures",
    stream: "futures",
    messageTypes: ["ticker", "trade"],
    description: "Kraken Futures — perpetual/futures ticker & trade tick data.",
  },
  "kraken-spot": {
    prefix: "kraken-spot",
    stream: "spot",
    messageTypes: ["ticker", "trade"],
    description: "Kraken Spot — spot-market ticker & trade tick data.",
  },
  "polymarket": {
    prefix: "polymarket",
    stream: "markets",
    messageTypes: ["book", "last_trade_price", "price_change", "best_bid_ask"],
    description: "Polymarket — CLOB order book, trades, and price changes.",
  },
  "hols": {
    prefix: "hols",
    stream: "crypto/daily",
    messageTypes: ["ohlcv"],
    description: "Crypto OHLCV bars — 1-minute & 5-minute candlesticks (open/high/low/close/volume) from Binance & Kraken, daily Parquet. (v14 model inputs.)",
  },
};

export interface FeedInfo {
  prefix: string;
  stream: string;
  messageTypes: string[];
  description: string;
}

/** Returns the human-friendly description for a feed, or an empty string if unknown. */
export function feedDescription(feed: string): string {
  return FEED_CONFIG[feed]?.description ?? "";
}

export interface ParquetFile {
  /** Full GCS path, e.g. "kalshi/kalshi/crypto/2026-02-15/ticker_0000.parquet" */
  path: string;
  /** Filename, e.g. "ticker_0000.parquet" */
  name: string;
  /** Message type, e.g. "ticker" */
  type: string;
  /** Time slot, e.g. "0000" */
  hour: string;
  /** File size in bytes */
  bytes: number;
}

export interface SignedFile extends ParquetFile {
  signedUrl: string;
  expiresAt: string; // ISO 8601
}

/**
 * List parquet files for a feed+date range, optionally filtered by message type.
 * GCS path pattern: {prefix}/{prefix}/{stream}/{date}/{type}_{HHMM}.parquet
 * The double-nesting ({prefix}/{prefix}/) is the actual layout from archiver sync.
 */
export async function listParquetFiles(
  bucket: string,
  feed: string,
  dateFrom: string,
  dateTo: string,
  msgType?: string,
): Promise<ParquetFile[]> {
  const config = FEED_CONFIG[feed];
  if (!config) {
    throw new Error(`Unknown feed: ${feed}. Valid feeds: ${Object.keys(FEED_CONFIG).join(", ")}`);
  }

  const storage = new Storage();
  const files: ParquetFile[] = [];

  // Iterate over each date in range
  const from = new Date(dateFrom);
  const to = new Date(dateTo);

  // HOLS uses a flat layout: {prefix}/{stream}/{date}/ohlcv.parquet
  // Archiver uses double-nesting: {prefix}/{prefix}/{stream}/{date}/{type}_{HHMM}.parquet
  const isFlat = config.prefix === "hols";

  for (let d = new Date(from); d <= to; d.setDate(d.getDate() + 1)) {
    const dateStr = d.toISOString().slice(0, 10);
    const gcsPrefix = isFlat
      ? `${config.prefix}/${config.stream}/${dateStr}/`
      : `${config.prefix}/${config.prefix}/${config.stream}/${dateStr}/`;

    const [gcsFiles] = await storage.bucket(bucket).getFiles({ prefix: gcsPrefix });

    for (const gcsFile of gcsFiles) {
      if (!gcsFile.name.endsWith(".parquet") && !gcsFile.name.endsWith(".csv")) continue;

      const fileName = gcsFile.name.split("/").pop() ?? "";
      const ext = fileName.endsWith(".csv") ? ".csv" : ".parquet";
      const baseName = fileName.replace(ext, "");

      let fileType: string;
      let hour: string;

      if (isFlat) {
        // Flat layout: ohlcv.parquet (no hour suffix)
        fileType = baseName;
        hour = dateStr;
      } else {
        // Archiver layout: ticker_0000.parquet
        const lastUnderscore = baseName.lastIndexOf("_");
        if (lastUnderscore === -1) continue;
        fileType = baseName.substring(0, lastUnderscore);
        hour = baseName.substring(lastUnderscore + 1);
      }

      // Filter by message type if specified
      if (msgType && fileType !== msgType) continue;

      const metadata = gcsFile.metadata;
      const bytes = Number(metadata.size ?? 0);

      files.push({
        path: gcsFile.name,
        name: fileName,
        type: fileType,
        hour,
        bytes,
      });
    }
  }

  return files;
}

/**
 * Generate V4 signed URLs for parquet files.
 * Max 12h expiration with Workload Identity signBlob.
 */
export async function generateSignedUrls(
  bucket: string,
  files: ParquetFile[],
  expiresInHours: number,
): Promise<SignedFile[]> {
  const storage = new Storage();
  const expiresMs = expiresInHours * 3600_000;
  const expiresAt = new Date(Date.now() + expiresMs);

  const results: SignedFile[] = [];

  for (const file of files) {
    // Prefix the downloaded filename with the date folder segment so multi-day
    // downloads don't collide (e.g. "2026-06-08-ohlcv-1m-binance.parquet").
    // file.path is a GCS object key like ".../{date}/{name}"; fall back to the
    // bare name if the expected date segment isn't present.
    const segments = file.path.split("/");
    const dateSegment = segments.length >= 2 ? segments[segments.length - 2] : "";
    const downloadName = /^\d{4}-\d{2}-\d{2}$/.test(dateSegment)
      ? `${dateSegment}-${file.name}`
      : file.name;

    const [signedUrl] = await storage
      .bucket(bucket)
      .file(file.path)
      .getSignedUrl({
        version: "v4",
        action: "read",
        expires: expiresAt,
        responseDisposition: `attachment; filename="${downloadName}"`,
      });

    results.push({
      ...file,
      signedUrl,
      expiresAt: expiresAt.toISOString(),
    });
  }

  return results;
}
