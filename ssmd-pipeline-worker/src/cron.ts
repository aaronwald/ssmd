import cronParser from "cron-parser";
const { parseExpression } = cronParser;
import type { CronPipeline } from "./db.ts";

/**
 * Determine whether a cron pipeline is due to fire.
 *
 * Algorithm: parse the cron expression from trigger_config.schedule,
 * find the most recent scheduled time before `now`, and check whether
 * it falls after `last_triggered_at`. If it does, the pipeline missed
 * that tick and should fire.
 *
 * Returns true when the pipeline should be triggered.
 */
/**
 * Compute the default date for a cron-triggered pipeline run.
 * Uses `date_offset_days` from trigger_config (default: -1, i.e. yesterday).
 */
export function computeCronDate(
  triggerConfig: { date_offset_days?: number },
  now: Date,
): string {
  const offsetDays = triggerConfig.date_offset_days ?? -1;
  return new Date(now.getTime() + offsetDays * 86_400_000)
    .toISOString()
    .slice(0, 10);
}

export function isCronDue(pipeline: CronPipeline, now: Date): boolean {
  const schedule = (pipeline.trigger_config as { schedule?: string }).schedule;
  if (!schedule) return false;

  try {
    const interval = parseExpression(schedule, {
      currentDate: now,
      tz: "UTC",
    });

    // prev() gives the most recent time the cron should have fired (at or before `now`)
    const prevTick = interval.prev().toDate();

    if (!pipeline.last_triggered_at) {
      // Never triggered — fire now
      return true;
    }

    const lastTriggered = new Date(pipeline.last_triggered_at);
    // If the most recent tick is after the last trigger, the pipeline is due
    return prevTick.getTime() > lastTriggered.getTime();
  } catch {
    // Invalid cron expression — skip silently (logged by caller)
    return false;
  }
}
