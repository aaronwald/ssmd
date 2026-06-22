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
    // HOLS is daily-aggregated: every message type uses the flat layout.
    flat: true,
    description: "Crypto OHLCV bars — 1-minute & 5-minute candlesticks (open/high/low/close/volume) from Binance & Kraken, daily Parquet. (v14 model inputs.)",
  },
  "massive": {
    prefix: "massive",
    stream: "equities",
    messageTypes: ["ohlcv_1s", "ohlcv_1m", "ohlcv_1d"],
    // Raw 1s/1m bars use the archiver double-nested layout
    // (massive/massive/equities/{date}/{type}_{HHMM}.parquet).
    // The daily aggregate (ohlcv_1d) is written by `hols aggregate --source massive`
    // to a flat path (massive/equities/daily/{date}/ohlcv-1d-massive.parquet).
    flatMessageTypes: ["ohlcv_1d"],
    flatStream: "equities/daily",
    description: "Massive (Polygon.io) US equities OHLCV bars — 1-second & 1-minute raw bars plus a daily (1d) aggregate.",
  },
};

export interface FeedInfo {
  prefix: string;
  stream: string;
  messageTypes: string[];
  description: string;
  /** When true, ALL message types use the flat layout: {prefix}/{stream}/{date}/{type}.parquet */
  flat?: boolean;
  /** Message types that use the flat layout while the rest use the nested archiver layout. */
  flatMessageTypes?: string[];
  /** Stream segment to use for flat-layout files (defaults to `stream` when omitted). */
  flatStream?: string;
}

/** True when files of the given message type use the flat (non-archiver) GCS layout. */
export function usesFlatLayout(config: FeedInfo, msgType?: string): boolean {
  if (config.flat) return true;
  if (msgType && config.flatMessageTypes?.includes(msgType)) return true;
  return false;
}

/**
 * Resolve the GCS directory prefix for a feed's files on a given date.
 * Flat layout:   {prefix}/{flatStream ?? stream}/{date}/
 * Nested layout: {prefix}/{prefix}/{stream}/{date}/   (archiver double-nesting)
 */
export function gcsDirPrefix(config: FeedInfo, dateStr: string, flat: boolean): string {
  // Path segments are required; a misconfigured feed must fail loudly rather
  // than produce a wrong (and silently empty) GCS prefix.
  if (!config.prefix) throw new Error("Feed config missing prefix");
  if (flat) {
    const stream = config.flatStream ?? config.stream;
    if (!stream) throw new Error(`Feed ${config.prefix} missing flat stream`);
    return `${config.prefix}/${stream}/${dateStr}/`;
  }
  if (!config.stream) throw new Error(`Feed ${config.prefix} missing stream`);
  return `${config.prefix}/${config.prefix}/${config.stream}/${dateStr}/`;
}

/**
 * Whether a feed has any files in the given layout (flat vs nested), used to
 * decide which directories to scan when no specific msgType is requested.
 */
export function scanLayout(config: FeedInfo, flat: boolean): boolean {
  if (flat) {
    return config.flat === true || (config.flatMessageTypes?.length ?? 0) > 0;
  }
  // Nested layout applies when the feed is not fully flat and at least one
  // message type is not in the flat set.
  if (config.flat === true) return false;
  const flatTypes = config.flatMessageTypes ?? [];
  return config.messageTypes.some((t) => !flatTypes.includes(t));
}

/**
 * Parse a parquet/csv basename into its message type and time-slot ("hour").
 * Flat layout files have no time-slot suffix (e.g. "ohlcv-1d-massive"); their
 * type is the whole basename and the hour is the date. Nested archiver files
 * are "{type}_{HHMM}". Returns null for nested files without an underscore so
 * malformed names are skipped (preserves prior behavior).
 */
export function parseFileType(
  baseName: string,
  flat: boolean,
  dateStr: string,
): { fileType: string; hour: string } | null {
  if (flat) {
    return { fileType: baseName, hour: dateStr };
  }
  const lastUnderscore = baseName.lastIndexOf("_");
  if (lastUnderscore === -1) return null;
  return {
    fileType: baseName.substring(0, lastUnderscore),
    hour: baseName.substring(lastUnderscore + 1),
  };
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

  // A feed may mix layouts (e.g. massive: raw 1s/1m bars use the archiver
  // double-nested layout, the daily ohlcv_1d aggregate is flat). When a msgType
  // is requested, scan only the layout that type uses; otherwise scan whichever
  // layouts the feed actually populates. The two layouts live under distinct
  // prefixes, so no dedupe is needed. gcsDirPrefix/parseFileType (defined above)
  // validate the feed config and fail loudly on a misconfigured feed.
  const layoutsToScan: boolean[] = msgType
    ? [usesFlatLayout(config, msgType)]
    : [true, false].filter((flat) => scanLayout(config, flat));

  for (let d = new Date(from); d <= to; d.setDate(d.getDate() + 1)) {
    const dateStr = d.toISOString().slice(0, 10);

    for (const flat of layoutsToScan) {
      const gcsPrefix = gcsDirPrefix(config, dateStr, flat);
      const [gcsFiles] = await storage.bucket(bucket).getFiles({ prefix: gcsPrefix });

      for (const gcsFile of gcsFiles) {
        if (!gcsFile.name.endsWith(".parquet") && !gcsFile.name.endsWith(".csv")) continue;

        const fileName = gcsFile.name.split("/").pop() ?? "";
        const ext = fileName.endsWith(".csv") ? ".csv" : ".parquet";
        const baseName = fileName.replace(ext, "");

        const parsed = parseFileType(baseName, flat, dateStr);
        if (!parsed) continue; // malformed nested name (no time-slot) — skip
        const { fileType, hour } = parsed;

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
