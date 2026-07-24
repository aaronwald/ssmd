/**
 * `ssmd retention lifecycle` — bound market_lifecycle_events by deleting rows older
 * than RETENTION_DAYS (default 30), per UTC date, ONLY after confirming that date's
 * lifecycle events are durable in GCS parquet. Fail-loud on any archive gap.
 */
import { getDb, deleteLifecycleEventsForDate, oldestLifecycleDate } from "../../lib/db/mod.ts";
import { getDateManifest } from "../../lib/gcs/catalog.ts";
import {
  cutoffDate,
  datesToDrop,
  manifestHasRows,
  type DateArchivedCheck,
} from "../../lib/retention/lifecycle.ts";

const BUCKET = "ssmd-data";
const DEFAULT_RETENTION_DAYS = 30;

export interface RetentionDeps {
  now: Date;
  retentionDays: number;
  oldest: string | null;
  check: DateArchivedCheck;
  del: (date: string) => Promise<number>;
}

/**
 * Orchestrate the per-date gate+delete. Injected deps make this unit-testable.
 * Throws (aborting the run) on the first date that fails the archive gate,
 * after deleting only strictly-older covered dates.
 */
export async function runLifecycleRetention(
  deps: RetentionDeps,
): Promise<{ dropped: number; datesDropped: string[] }> {
  const { now, retentionDays, oldest, check, del } = deps;
  if (!oldest) return { dropped: 0, datesDropped: [] };

  const cutoff = cutoffDate(now, retentionDays);
  const dates = datesToDrop(oldest, cutoff);

  let dropped = 0;
  const datesDropped: string[] = [];
  for (const date of dates) {
    const archived = await check(date);
    if (!archived) {
      throw new Error(
        `lifecycle retention: date ${date} not archived to GCS parquet; aborting`,
      );
    }
    dropped += await del(date);
    datesDropped.push(date);
  }
  return { dropped, datesDropped };
}

/** Production archive check: the date's lifecycle parquet manifest exists with rows. */
function gcsLifecycleCheck(): DateArchivedCheck {
  return async (date: string) => {
    const manifest = await getDateManifest(BUCKET, "kalshi", "lifecycle", "kalshi", date);
    return manifestHasRows(manifest);
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
  if (!Number.isFinite(retentionDays) || retentionDays < 1) {
    console.error(`Invalid RETENTION_DAYS: ${rawDays}`);
    Deno.exit(1);
  }

  const db = getDb();
  try {
    const oldest = await oldestLifecycleDate(db);
    const result = await runLifecycleRetention({
      now: new Date(),
      retentionDays,
      oldest,
      check: gcsLifecycleCheck(),
      del: (date) => deleteLifecycleEventsForDate(db, date),
    });
    console.log(
      `lifecycle retention: dropped ${result.dropped} rows across ${result.datesDropped.length} dates (< ${retentionDays}d) ` +
        (result.datesDropped.length ? `[${result.datesDropped[0]}..${result.datesDropped.at(-1)}]` : "[none]"),
    );
  } catch (err) {
    // Fail loud: archive gap or DB error -> non-zero exit -> K8s marks the Job failed -> alert.
    console.error(`lifecycle retention FAILED: ${(err as Error).message}`);
    Deno.exit(1);
  }
}
