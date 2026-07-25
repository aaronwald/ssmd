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
  /** Hourly slot HHMM = base name after the last underscore, e.g. "1400". */
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

/**
 * Resolve the UTC date that Phase 2 (GCS) scoring should scan.
 *
 * Phase 2's denominators are whole-day constants (DQ_ACTIVE_HOURS = 24,
 * expectedSlots = 24, one hourly parquet file per hour), so they are only valid
 * against a COMPLETE day. The health CronJob runs at 08:30 UTC, so scanning
 * "today" caps both uptime and coverage at roughly 33% (only ~8 of 24 hourly
 * slots have landed by 08:30 UTC) before any real data problem. Default to the
 * previous UTC day.
 *
 * @param now Current time (injected so this stays pure and testable).
 * @param override Optional explicit `YYYY-MM-DD` (the `--date` flag).
 * @throws Error when `override` is not a real `YYYY-MM-DD` calendar date.
 */
export function resolveScanDate(now: Date, override?: string): string {
  if (override !== undefined) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(override)) {
      throw new Error(`--date must be YYYY-MM-DD, got: ${override}`);
    }
    const parsed = new Date(`${override}T00:00:00Z`);
    if (Number.isNaN(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== override) {
      throw new Error(`--date is not a valid calendar date: ${override}`);
    }
    return override;
  }
  const prev = new Date(now.getTime());
  prev.setUTCDate(prev.getUTCDate() - 1);
  return prev.toISOString().slice(0, 10);
}
