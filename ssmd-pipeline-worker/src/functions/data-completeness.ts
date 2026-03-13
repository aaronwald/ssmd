import type { CodeInput, CodeOutput } from "./mod.ts";

/**
 * data-completeness: validates parquet file counts and record ranges per feed.
 *
 * Expects previous HTTP stages that called /v1/data/check-freshness (for file
 * existence) and /v1/hols/validate (for record counts). Each stage output is
 * { status, body, truncated }.
 *
 * Params:
 *   freshnessStageIndex (number, default 0)
 *   validateStageIndex (number, default 1)
 *   minRecords (number, default 1000) — minimum total records per source
 */
export function dataCompleteness(input: CodeInput): CodeOutput {
  const freshIdx = (input.params?.freshnessStageIndex as number) ?? 0;
  const valIdx = (input.params?.validateStageIndex as number) ?? 1;
  const minRecords = (input.params?.minRecords as number) ?? 1000;

  const issues: string[] = [];

  // Check freshness data
  const freshStage = input.stages[freshIdx] as { output?: string } | undefined;
  if (freshStage?.output) {
    try {
      const parsed = JSON.parse(freshStage.output);
      const feeds = parsed?.body?.feeds as Array<{
        feed: string; status: string; newest_date: string; age_hours: number; stale: boolean;
      }> | undefined;

      if (!feeds || !Array.isArray(feeds)) {
        issues.push("Freshness response missing feeds array");
      } else {
        for (const f of feeds) {
          if (f.stale) {
            issues.push(`${f.feed}: stale (${f.age_hours.toFixed(1)}h old)`);
          }
          if (f.status !== "fresh") {
            issues.push(`${f.feed}: status=${f.status}`);
          }
        }
      }
    } catch {
      issues.push("Failed to parse freshness data");
    }
  } else {
    issues.push("No freshness stage output");
  }

  // Check validation data (HOLS validate endpoint)
  const valStage = input.stages[valIdx] as { output?: string } | undefined;
  if (valStage?.output) {
    try {
      const parsed = JSON.parse(valStage.output);
      const body = parsed?.body;

      // Check each source section (HOLS validate returns rest, ws, binance_5m, binance_1m)
      for (const source of ["rest", "ws", "binance_5m", "binance_1m"]) {
        const section = body?.[source];
        if (!section) continue;

        const totalRecords = section.total_rows ?? section.total_records ?? section.total_bars ?? 0;
        if (totalRecords < minRecords) {
          issues.push(`${source}: only ${totalRecords} records (min ${minRecords})`);
        }

        const tickers = section.unique_tickers ?? section.ticker_count ?? section.tickers ?? 0;
        if (tickers === 0) {
          issues.push(`${source}: zero tickers`);
        }
      }
    } catch {
      issues.push("Failed to parse validation data");
    }
  }
  // validation stage is optional — only HOLS pipelines have it

  const allGood = issues.length === 0;

  return {
    result: {
      allGood,
      issueCount: issues.length,
      issues,
      summary: allGood
        ? "All feeds complete — files present, records within range"
        : `${issues.length} issue(s): ${issues.join("; ")}`,
    },
    skip: allGood,
  };
}
