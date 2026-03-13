import type { CodeInput, CodeOutput } from "./mod.ts";

/**
 * kxbtcd-canary: detects KXBTCD hourly market visibility issues.
 *
 * Compares secmaster DB state against Redis/monitor API state to catch:
 * - Too few active KXBTCD events (should be ~24 hourly events)
 * - Events missing from Redis that exist in DB (cache/CDC gap)
 * - Events with 0 active markets despite future close_time (the settled-status bug)
 *
 * Stage inputs:
 *   Stage 0 (sql): Active KXBTCD market counts per event from secmaster DB
 *   Stage 1 (http): Monitor events from Redis via /v1/monitor/events?series=KXBTCD
 *
 * Params:
 *   sqlStageIndex (number, default 0) — position of the SQL stage
 *   httpStageIndex (number, default 1) — position of the HTTP stage
 *   minActiveEvents (number, default 10) — minimum expected active events with future close_time
 */

interface SqlRow {
  event_ticker: string;
  active_markets: number;
  total_markets: number;
  earliest_close: string;
  latest_close: string;
}

interface MonitorEvent {
  ticker: string;
  [key: string]: unknown;
}

export function kxbtcdCanary(input: CodeInput): CodeOutput {
  const sqlIdx = (input.params?.sqlStageIndex as number) ?? 0;
  const httpIdx = (input.params?.httpStageIndex as number) ?? 1;
  const minActiveEvents = (input.params?.minActiveEvents as number) ?? 10;

  const issues: string[] = [];

  // Parse SQL stage output (active KXBTCD markets grouped by event)
  const sqlStage = input.stages[sqlIdx] as { output?: string } | undefined;
  if (!sqlStage?.output) {
    return { result: { error: "No SQL stage output found" }, skip: false };
  }

  let dbEvents: SqlRow[];
  try {
    const parsed = JSON.parse(sqlStage.output);
    // SQL stage output is { rows: [...], row_count: N }
    dbEvents = (parsed?.rows ?? parsed) as SqlRow[];
    if (!Array.isArray(dbEvents)) {
      return { result: { error: "SQL output is not an array", raw: parsed }, skip: false };
    }
  } catch {
    return { result: { error: "Failed to parse SQL stage output" }, skip: false };
  }

  // Parse HTTP stage output (monitor events from Redis)
  const httpStage = input.stages[httpIdx] as { output?: string } | undefined;
  if (!httpStage?.output) {
    return { result: { error: "No HTTP stage output found" }, skip: false };
  }

  let redisEvents: MonitorEvent[];
  try {
    const parsed = JSON.parse(httpStage.output);
    // HTTP stage output is { status, body: { events: [...] }, truncated }
    redisEvents = (parsed?.body?.events ?? []) as MonitorEvent[];
    if (!Array.isArray(redisEvents)) {
      return { result: { error: "Monitor events response is not an array", raw: parsed }, skip: false };
    }
  } catch {
    return { result: { error: "Failed to parse HTTP stage output" }, skip: false };
  }

  // Check 1: Enough active events in DB
  const activeDbEvents = dbEvents.filter((e) => e.active_markets > 0);
  if (activeDbEvents.length < minActiveEvents) {
    issues.push(
      `DB has only ${activeDbEvents.length} active KXBTCD events (expected >= ${minActiveEvents}). ` +
      `Total events with future close_time: ${dbEvents.length}`,
    );
  }

  // Check 2: Events with 0 active markets despite having future close_time (the settled-status bug)
  const zeroActiveEvents = dbEvents.filter((e) => e.active_markets === 0);
  if (zeroActiveEvents.length > 0) {
    issues.push(
      `${zeroActiveEvents.length} event(s) have 0 active markets despite future close_time: ` +
      zeroActiveEvents.map((e) => e.event_ticker).join(", "),
    );
  }

  // Check 3: Redis vs DB gap — events in DB but missing from Redis
  const redisEventTickers = new Set(redisEvents.map((e) => e.ticker));
  const dbEventTickers = dbEvents.map((e) => e.event_ticker);
  const missingFromRedis = dbEventTickers.filter((t) => !redisEventTickers.has(t));
  if (missingFromRedis.length > 0) {
    issues.push(
      `${missingFromRedis.length} event(s) in DB but missing from Redis: ` +
      missingFromRedis.join(", "),
    );
  }

  // Check 4: Redis has events not in DB (stale cache)
  const dbEventTickerSet = new Set(dbEventTickers);
  const extraInRedis = redisEvents
    .map((e) => e.ticker)
    .filter((t) => !dbEventTickerSet.has(t));
  if (extraInRedis.length > 0) {
    // Not an issue per se, but worth noting — could be stale cache entries
    // Don't add as issue, just include in summary
  }

  const healthy = issues.length === 0;

  return {
    result: {
      healthy,
      dbEventCount: dbEvents.length,
      dbActiveEventCount: activeDbEvents.length,
      redisEventCount: redisEvents.length,
      zeroActiveEvents: zeroActiveEvents.map((e) => e.event_ticker),
      missingFromRedis,
      extraInRedis,
      issues,
      summary: healthy
        ? `KXBTCD healthy: ${activeDbEvents.length} active events in DB, ${redisEvents.length} in Redis`
        : `${issues.length} issue(s): ${issues.join("; ")}`,
    },
    skip: healthy,
  };
}
