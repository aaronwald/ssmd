/**
 * GCS catalog reader — reads catalog.json and per-date parquet-manifest.json from GCS.
 * Types mirror the Rust catalog/manifest structs.
 * In-memory cache with 5-minute TTL (catalog changes once daily at 02:00 UTC).
 */
import { Storage } from "@google-cloud/storage";

// --- Types matching Rust structs ---

export interface ColumnDef {
  name: string;
  arrow_type: string;
  nullable: boolean;
}

export interface SchemaInfo {
  schema_name: string;
  schema_version: string;
  columns: ColumnDef[];
}

export interface FeedSummary {
  feed: string;
  stream: string;
  prefix: string;
  message_types: string[];
  date_min: string;
  date_max: string;
  total_files: number;
  total_bytes: number;
  total_rows: number;
  dates: string[];
  schemas: Record<string, SchemaInfo>;
}

export interface Catalog {
  generated_at: string;
  version: string;
  feeds: FeedSummary[];
}

/** Per-file entry in v2.0.0 manifests */
export interface ParquetFileEntry {
  path: string;
  message_type: string;
  hour: string;
  bytes: number;
  row_count: number;
  schema_name: string;
  schema_version: string;
}

/** Per-hour or aggregate stats */
export interface ManifestStats {
  files_read: number;
  lines_total: number;
  lines_empty: number;
  lines_json_error: number;
  lines_type_unknown: number;
  lines_no_schema: Record<string, number>;
  parse_batch_input: Record<string, number>;
  parse_batch_dropped: Record<string, number>;
  records_written: Record<string, number>;
}

export interface ParquetManifest {
  feed: string;
  stream: string;
  date: string;
  generated_at: string;
  version: string;
  hours: Record<string, ManifestStats>;
  totals: ManifestStats;
  files?: ParquetFileEntry[];
  schemas?: Record<string, SchemaInfo>;
}

// --- Cache ---

interface CacheEntry<T> {
  data: T;
  expiresAt: number;
}

const CACHE_TTL_MS = 5 * 60 * 1000; // 5 minutes
let catalogCache: CacheEntry<Catalog> | null = null;

// --- Public API ---

/**
 * Read the root catalog.json from GCS. Returns null if not found.
 * Cached for 5 minutes.
 */
export async function getCatalog(bucket: string): Promise<Catalog | null> {
  if (catalogCache && Date.now() < catalogCache.expiresAt) {
    return catalogCache.data;
  }

  const storage = new Storage();
  try {
    const [content] = await storage.bucket(bucket).file("catalog.json").download();
    const catalog: Catalog = JSON.parse(content.toString("utf-8"));
    catalogCache = { data: catalog, expiresAt: Date.now() + CACHE_TTL_MS };
    return catalog;
  } catch (err: unknown) {
    const error = err as { code?: number };
    if (error.code === 404) {
      return null;
    }
    throw err;
  }
}

/**
 * Read a per-date parquet-manifest.json from GCS. Not cached (called infrequently).
 */
export async function getDateManifest(
  bucket: string,
  feed: string,
  stream: string,
  prefix: string,
  date: string,
): Promise<ParquetManifest | null> {
  const storage = new Storage();
  const path = `${prefix}/${prefix}/${stream}/${date}/parquet-manifest.json`;
  try {
    const [content] = await storage.bucket(bucket).file(path).download();
    return JSON.parse(content.toString("utf-8"));
  } catch (err: unknown) {
    const error = err as { code?: number };
    if (error.code === 404) {
      return null;
    }
    throw err;
  }
}
