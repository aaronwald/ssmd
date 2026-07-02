/**
 * Pure (dependency-free) GCS filename parsing for the health command's Phase 2
 * GCS scans. Kept out of health.ts so it can be unit-tested without the
 * @google-cloud/storage native client.
 */

export interface GcsFileInfo {
  /** Full gs:// path (set by the caller that lists the bucket). */
  path: string;
  /** Object base name, e.g. "ticker_1400.parquet". */
  name: string;
  sizeBytes: number;
  /** Message type = base name before the last underscore, e.g. "ticker". */
  msgType: string;
  /** 15-minute slot HHMM = base name after the last underscore, e.g. "1400". */
  time: string;
}

/**
 * Parse a GCS object base name of the form `{msgType}_{HHMM}.{ext}` into its
 * parts (without `path` — the caller adds the full gs:// path). Returns null for
 * names that don't match the expected extension or have no underscore separator
 * (e.g. manifest.json), so non-data objects are skipped rather than mis-parsed.
 */
export function parseGcsFileInfo(
  name: string,
  sizeBytes: number,
  ext = "parquet",
): Omit<GcsFileInfo, "path"> | null {
  if (!name || !name.endsWith(`.${ext}`)) return null;
  const base = name.slice(0, name.length - (ext.length + 1)); // strip ".{ext}"
  const lastUnderscore = base.lastIndexOf("_");
  if (lastUnderscore === -1) return null;
  const msgType = base.substring(0, lastUnderscore);
  const time = base.substring(lastUnderscore + 1);
  if (!msgType || !time) return null;
  return {
    name,
    sizeBytes: Number.isFinite(sizeBytes) ? sizeBytes : 0,
    msgType,
    time,
  };
}
