import type { CodeInput, CodeOutput } from "./mod.ts";

/**
 * parquet-quality: validates parquet data quality from SQL query results.
 *
 * Expects a previous SQL stage that queries parquet stats (null rates,
 * duplicate counts, schema mismatches). The SQL stage output is the
 * query result rows.
 *
 * Params:
 *   sqlStageIndex (number, default 0) — position of the SQL stage
 *   maxNullRatePct (number, default 5) — max acceptable null rate percentage
 *   maxDuplicates (number, default 0) — max acceptable duplicate count
 */
export function parquetQuality(input: CodeInput): CodeOutput {
  const sqlIdx = (input.params?.sqlStageIndex as number) ?? 0;
  const maxNullRate = (input.params?.maxNullRatePct as number) ?? 5;
  const maxDupes = (input.params?.maxDuplicates as number) ?? 0;

  const stage = input.stages[sqlIdx] as { output?: string } | undefined;
  if (!stage?.output) {
    return { result: { error: "No SQL stage output found" }, skip: false };
  }

  let rows: Array<Record<string, unknown>>;
  try {
    const parsed = JSON.parse(stage.output);
    // SQL stage output is { rows: [...] } or directly an array
    rows = Array.isArray(parsed) ? parsed : (parsed?.rows ?? parsed?.body ?? []);
    if (!Array.isArray(rows)) rows = [];
  } catch {
    return { result: { error: "Failed to parse SQL output" }, skip: false };
  }

  if (rows.length === 0) {
    return {
      result: { error: "No parquet stats rows returned — data may be missing" },
      skip: false,
    };
  }

  const issues: string[] = [];
  const feedStats: Record<string, { nullIssues: string[]; dupeCount: number; recordCount: number }> = {};

  for (const row of rows) {
    const feed = String(row.feed ?? row.source ?? "unknown");
    if (!feedStats[feed]) {
      feedStats[feed] = { nullIssues: [], dupeCount: 0, recordCount: 0 };
    }
    const stats = feedStats[feed];

    // Record count
    const records = Number(row.record_count ?? row.total_records ?? 0);
    stats.recordCount += records;

    // Null rate check
    const nullRate = Number(row.null_rate_pct ?? row.null_rate ?? 0);
    const column = String(row.column_name ?? row.field ?? "");
    if (nullRate > maxNullRate && column) {
      stats.nullIssues.push(`${column}: ${nullRate.toFixed(1)}% null`);
      issues.push(`${feed}/${column}: ${nullRate.toFixed(1)}% null (max ${maxNullRate}%)`);
    }

    // Duplicate check
    const dupes = Number(row.duplicate_count ?? row.duplicates ?? 0);
    stats.dupeCount += dupes;
    if (dupes > maxDupes) {
      issues.push(`${feed}: ${dupes} duplicates (max ${maxDupes})`);
    }
  }

  const allGood = issues.length === 0;

  return {
    result: {
      allGood,
      issueCount: issues.length,
      issues,
      feedStats,
      summary: allGood
        ? `Parquet quality OK — ${rows.length} stats checked across ${Object.keys(feedStats).length} feeds`
        : `${issues.length} quality issue(s): ${issues.join("; ")}`,
    },
    skip: allGood,
  };
}
