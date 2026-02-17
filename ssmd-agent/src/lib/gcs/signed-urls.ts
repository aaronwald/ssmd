/**
 * GCS signed URL generation for parquet data sharing.
 * Uses Workload Identity (signBlob) for V4 signed URLs.
 */
import { Storage } from "@google-cloud/storage";

/** Feed → GCS path mapping (matches parquet-gen CronJob layout) */
export const FEED_CONFIG: Record<string, FeedInfo> = {
  "kalshi": { prefix: "kalshi", stream: "crypto", messageTypes: ["ticker", "trade", "market_lifecycle_v2"] },
  "kraken-futures": { prefix: "kraken-futures", stream: "futures", messageTypes: ["ticker", "trade"] },
  "polymarket": { prefix: "polymarket", stream: "markets", messageTypes: ["book", "last_trade_price", "price_change", "best_bid_ask"] },
};

export interface FeedInfo {
  prefix: string;
  stream: string;
  messageTypes: string[];
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

  for (let d = new Date(from); d <= to; d.setDate(d.getDate() + 1)) {
    const dateStr = d.toISOString().slice(0, 10);
    const prefix = `${config.prefix}/${config.prefix}/${config.stream}/${dateStr}/`;

    const [gcsFiles] = await storage.bucket(bucket).getFiles({ prefix });

    for (const gcsFile of gcsFiles) {
      if (!gcsFile.name.endsWith(".parquet")) continue;

      const fileName = gcsFile.name.split("/").pop() ?? "";
      const baseName = fileName.replace(".parquet", "");
      const lastUnderscore = baseName.lastIndexOf("_");
      if (lastUnderscore === -1) continue;

      const fileType = baseName.substring(0, lastUnderscore);
      const hour = baseName.substring(lastUnderscore + 1);

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
    const [signedUrl] = await storage
      .bucket(bucket)
      .file(file.path)
      .getSignedUrl({
        version: "v4",
        action: "read",
        expires: expiresAt,
      });

    results.push({
      ...file,
      signedUrl,
      expiresAt: expiresAt.toISOString(),
    });
  }

  return results;
}
