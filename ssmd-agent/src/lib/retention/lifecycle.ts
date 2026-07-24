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

/** The Kalshi lifecycle message type — the only records_written key retention gates on. */
export const LIFECYCLE_MESSAGE_TYPE = "market_lifecycle_v2";

/**
 * True iff the manifest proves the lifecycle events for its date are COMPLETELY and
 * CLEANLY archived to parquet — the only safe basis for deleting that day's DB rows.
 *
 * Requires, for the lifecycle message type specifically (not summed across types):
 *  - the manifest exists, and
 *  - `records_written[market_lifecycle_v2] > 0` (something was actually written), and
 *  - `parse_batch_dropped[market_lifecycle_v2] == 0` (no lifecycle records were dropped), and
 *  - no stream-level parse failures (`lines_json_error == 0`, `lines_type_unknown == 0`).
 *
 * Rejecting on dropped/errored records is the guard against the partial-archive trap: an
 * archiver run that wrote 5 of 1000 records reports records_written>0 but also
 * parse_batch_dropped>0 — treating that as "covered" and deleting the full DB day would
 * lose the 995 unarchived events (they exist in neither GCS parquet nor the DB). Fail loud
 * instead: the operator re-runs parquet-gen for that date, then retention proceeds.
 */
export function manifestCoversLifecycle(m: ParquetManifest | null): boolean {
  if (!m || !m.totals) return false;
  const t = m.totals;
  const written = t.records_written?.[LIFECYCLE_MESSAGE_TYPE] ?? 0;
  if (written <= 0) return false;
  const dropped = t.parse_batch_dropped?.[LIFECYCLE_MESSAGE_TYPE] ?? 0;
  if (dropped > 0) return false;
  if ((t.lines_json_error ?? 0) > 0) return false;
  if ((t.lines_type_unknown ?? 0) > 0) return false;
  // A lifecycle line the parquet-gen couldn't match to a schema was NOT archived, even
  // though records_written may be >0 for other lines — same partial-archive class, so any
  // lines_no_schema entry blocks the delete.
  const noSchema = Object.values(t.lines_no_schema ?? {}).reduce((a, b) => a + (b ?? 0), 0);
  if (noSchema > 0) return false;
  return true;
}
