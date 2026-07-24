/**
 * Pure logic for market_lifecycle_events retention.
 * I/O (GCS manifest reads, DB deletes) is injected so this is unit-testable.
 */
import type { ParquetManifest } from "../gcs/catalog.ts";

/** A check that resolves true iff `date`'s lifecycle events are durable in GCS parquet. */
export type DateArchivedCheck = (date: string) => Promise<boolean>;

/** UTC YYYY-MM-DD that is `retentionDays` before `now`. Dates strictly before this are droppable. */
export function cutoffDate(now: Date, retentionDays: number): string {
  const ms = now.getTime() - retentionDays * 24 * 60 * 60 * 1000;
  return new Date(ms).toISOString().slice(0, 10);
}

/** Inclusive list of YYYY-MM-DD from `oldest` up to (but excluding) `cutoff`. */
export function datesToDrop(oldest: string, cutoff: string): string[] {
  const out: string[] = [];
  const oneDay = 24 * 60 * 60 * 1000;
  let t = Date.parse(`${oldest}T00:00:00Z`);
  const end = Date.parse(`${cutoff}T00:00:00Z`);
  while (t < end) {
    out.push(new Date(t).toISOString().slice(0, 10));
    t += oneDay;
  }
  return out;
}

/** True iff the manifest exists and reports at least one written record across message types. */
export function manifestHasRows(m: ParquetManifest | null): boolean {
  if (!m) return false;
  const written = m.totals?.records_written ?? {};
  return Object.values(written).reduce((a, b) => a + (b ?? 0), 0) > 0;
}

/**
 * Resolve iff EVERY date passes `check`. Throw on the first date that does not,
 * before the caller deletes anything for it. Fail-loud: never drop unarchived data.
 */
export async function assertDatesArchived(
  dates: string[],
  check: DateArchivedCheck,
): Promise<void> {
  for (const date of dates) {
    const ok = await check(date);
    if (!ok) {
      throw new Error(
        `lifecycle retention: date ${date} not archived to GCS parquet; aborting`,
      );
    }
  }
}
