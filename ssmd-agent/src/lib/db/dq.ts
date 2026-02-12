/**
 * Data quality database operations (Drizzle ORM)
 */
import { desc, sql } from "drizzle-orm";
import { type Database } from "./client.ts";
import { dqDailyScores } from "./schema.ts";

/**
 * Derive a letter grade from a numeric score.
 */
function scoreToGrade(score: number): string {
  if (score >= 90) return "A";
  if (score >= 80) return "B";
  if (score >= 70) return "C";
  if (score >= 60) return "D";
  return "F";
}

/**
 * List daily DQ scores with optional filters.
 */
export async function listDailyScores(
  db: Database,
  options: {
    feed?: string;
    from?: string;
    to?: string;
    limit?: number;
  } = {},
): Promise<{
  date: string;
  feed: string;
  score: number;
  composite_score: number | null;
  grade: string;
  gap_count: number | null;
  coverage_pct: number | null;
  expected_messages: number | null;
  actual_messages: number | null;
  // deno-lint-ignore no-explicit-any
  details: any;
}[]> {
  const limit = options.limit ?? 100;

  const conditions: ReturnType<typeof sql>[] = [];

  if (options.feed) {
    conditions.push(sql`${dqDailyScores.feed} = ${options.feed}`);
  }
  if (options.from) {
    conditions.push(sql`${dqDailyScores.checkDate} >= ${options.from}`);
  }
  if (options.to) {
    conditions.push(sql`${dqDailyScores.checkDate} <= ${options.to}`);
  }

  const whereClause = conditions.length > 0
    ? sql.join(conditions, sql` AND `)
    : sql`TRUE`;

  const rows = await db
    .select()
    .from(dqDailyScores)
    .where(whereClause)
    .orderBy(desc(dqDailyScores.checkDate), dqDailyScores.feed)
    .limit(limit);

  return rows.map((r) => {
    const score = Number(r.score);
    return {
      date: r.checkDate,
      feed: r.feed,
      score,
      composite_score: r.compositeScore ? Number(r.compositeScore) : null,
      grade: scoreToGrade(r.compositeScore ? Number(r.compositeScore) : score),
      gap_count: r.gapCount,
      coverage_pct: r.coveragePct ? Number(r.coveragePct) : null,
      expected_messages: r.expectedMessages,
      actual_messages: r.actualMessages,
      details: r.details,
    };
  });
}

/**
 * Compute SLA metrics per feed over a configurable window.
 * Aggregates from dq_daily_scores using coverage_pct and details JSONB.
 */
export async function getSlaMetrics(
  db: Database,
  options: {
    windowDays?: number;
  } = {},
): Promise<{
  feed: string;
  uptime_pct: number;
  avg_latency_ms: number | null;
  freshness_minutes: number | null;
  gap_count: number;
  coverage_pct: number;
  days_measured: number;
}[]> {
  const windowDays = options.windowDays ?? 7;

  const rows = await db.execute(
    sql`SELECT
      feed,
      COUNT(*)::int AS days_measured,
      ROUND(AVG(COALESCE(coverage_pct, score))::numeric, 2) AS avg_coverage_pct,
      COALESCE(SUM(gap_count), 0)::int AS total_gap_count,
      ROUND(AVG((details->>'avg_latency_ms')::numeric), 2) AS avg_latency_ms,
      ROUND(AVG((details->>'freshness_minutes')::numeric), 2) AS avg_freshness_minutes
    FROM dq_daily_scores
    WHERE check_date >= CURRENT_DATE - ${windowDays}::int
    GROUP BY feed
    ORDER BY feed`,
  );

  return rows.map((r) => ({
    feed: r.feed as string,
    uptime_pct: Number(r.avg_coverage_pct ?? 0),
    avg_latency_ms: r.avg_latency_ms != null ? Number(r.avg_latency_ms) : null,
    freshness_minutes: r.avg_freshness_minutes != null ? Number(r.avg_freshness_minutes) : null,
    gap_count: Number(r.total_gap_count ?? 0),
    coverage_pct: Number(r.avg_coverage_pct ?? 0),
    days_measured: Number(r.days_measured),
  }));
}

/**
 * Extract gap reports from dq_daily_scores details JSONB for a specific feed.
 * The details field may contain a "gaps" array with {start, end, duration_minutes}.
 */
export async function getGapReports(
  db: Database,
  options: {
    feed: string;
    from?: string;
    to?: string;
  },
): Promise<{
  start: string;
  end: string;
  duration_minutes: number;
  feed: string;
  date: string;
}[]> {
  const rows = await db.execute(
    sql`SELECT
      check_date,
      feed,
      gap->>'start' AS gap_start,
      gap->>'end' AS gap_end,
      (gap->>'duration_minutes')::numeric AS duration_minutes
    FROM dq_daily_scores,
      jsonb_array_elements(COALESCE(details->'gaps', '[]'::jsonb)) AS gap
    WHERE feed = ${options.feed}
      ${options.from ? sql`AND check_date >= ${options.from}` : sql``}
      ${options.to ? sql`AND check_date <= ${options.to}` : sql``}
    ORDER BY gap->>'start' DESC`,
  );

  return rows.map((r) => ({
    start: r.gap_start as string,
    end: r.gap_end as string,
    duration_minutes: Number(r.duration_minutes),
    feed: r.feed as string,
    date: r.check_date as string,
  }));
}
