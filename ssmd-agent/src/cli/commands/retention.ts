/**
 * `ssmd retention lifecycle` — bound market_lifecycle_events by deleting rows older
 * than RETENTION_DAYS (default 30), per UTC date, ONLY after confirming that date's
 * lifecycle events are durable in GCS parquet. Fail-loud on any archive gap.
 */
import {
  getDb,
  deleteLifecycleEventsForDate,
  countLifecycleEventsForDate,
  oldestLifecycleDate,
} from "../../lib/db/mod.ts";
import { getDateManifest } from "../../lib/gcs/catalog.ts";
import {
  cutoffDate,
  datesToDrop,
  manifestCoversLifecycle,
  type DateArchivedCheck,
} from "../../lib/retention/lifecycle.ts";

const BUCKET = "ssmd-data";
const DEFAULT_RETENTION_DAYS = 30;

export interface RetentionDeps {
  now: Date;
  retentionDays: number;
  oldest: string | null;
  /** DB row count for a UTC date — used to skip legitimately-empty days without gating. */
  count: (date: string) => Promise<number>;
  /** Archive gate: true iff the date's lifecycle events are completely & cleanly in GCS parquet. */
  check: DateArchivedCheck;
  del: (date: string) => Promise<number>;
}

/**
 * Orchestrate the per-date count → gate → delete. Injected deps make this unit-testable.
 *
 * For each UTC date older than the window, oldest→newest:
 *  - if the DB has zero rows for the date, SKIP it (nothing to protect or delete; a
 *    legitimately-empty day has no archive manifest and must not wedge the gate), else
 *  - require the archive gate to pass; on the FIRST failing date, throw and abort the run
 *    (only strictly-older covered dates have been deleted — never an uncovered date), else
 *  - delete that date's rows.
 */
export async function runLifecycleRetention(
  deps: RetentionDeps,
): Promise<{ dropped: number; datesDropped: string[]; datesSkipped: string[] }> {
  const { now, retentionDays, oldest, count, check, del } = deps;
  if (!oldest) return { dropped: 0, datesDropped: [], datesSkipped: [] };

  const cutoff = cutoffDate(now, retentionDays);
  const dates = datesToDrop(oldest, cutoff);

  let dropped = 0;
  const datesDropped: string[] = [];
  const datesSkipped: string[] = [];
  for (const date of dates) {
    const rows = await count(date);
    if (rows === 0) {
      // No rows for this UTC day → nothing to archive-protect or delete. Skipping avoids
      // an empty feed-outage day (which has no parquet manifest) permanently wedging the gate.
      datesSkipped.push(date);
      continue;
    }
    const archived = await check(date);
    if (!archived) {
      throw new Error(
        `lifecycle retention: date ${date} not completely archived to GCS parquet; aborting`,
      );
    }
    dropped += await del(date);
    datesDropped.push(date);
  }
  return { dropped, datesDropped, datesSkipped };
}

/**
 * Production archive check: the date's lifecycle parquet manifest must prove a COMPLETE,
 * CLEAN archive of the lifecycle message type (records written, none dropped, no parse
 * errors) — a missing/partial/errored manifest fails the gate so retention refuses to delete.
 */
function gcsLifecycleCheck(): DateArchivedCheck {
  return async (date: string) => {
    const manifest = await getDateManifest(BUCKET, "kalshi", "lifecycle", "kalshi", date);
    return manifestCoversLifecycle(manifest);
  };
}

export async function handleRetention(
  subcommand: string,
  _flags: unknown,
): Promise<void> {
  if (subcommand !== "lifecycle") {
    console.error(`Unknown retention target: ${subcommand} (expected 'lifecycle')`);
    Deno.exit(1);
  }

  const rawDays = Deno.env.get("RETENTION_DAYS");
  const retentionDays = rawDays === undefined ? DEFAULT_RETENTION_DAYS : Number(rawDays);
  if (!Number.isInteger(retentionDays) || retentionDays < 1) {
    console.error(`Invalid RETENTION_DAYS (expected a positive integer): ${rawDays}`);
    Deno.exit(1);
  }

  const db = getDb();
  try {
    const oldest = await oldestLifecycleDate(db);
    const result = await runLifecycleRetention({
      now: new Date(),
      retentionDays,
      oldest,
      count: (date) => countLifecycleEventsForDate(db, date),
      check: gcsLifecycleCheck(),
      del: (date) => deleteLifecycleEventsForDate(db, date),
    });
    console.log(
      `lifecycle retention: dropped ${result.dropped} rows across ${result.datesDropped.length} dates ` +
        `(< ${retentionDays}d), skipped ${result.datesSkipped.length} empty dates ` +
        (result.datesDropped.length ? `[${result.datesDropped[0]}..${result.datesDropped.at(-1)}]` : "[none]"),
    );
  } catch (err) {
    // Fail loud: archive gap or DB error -> non-zero exit -> K8s marks the Job failed -> alert.
    console.error(`lifecycle retention FAILED: ${(err as Error).message}`);
    Deno.exit(1);
  }
}
