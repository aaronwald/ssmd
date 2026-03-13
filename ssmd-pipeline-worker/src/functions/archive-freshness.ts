import type { CodeInput, CodeOutput } from "./mod.ts";

/**
 * archive-freshness: validates all feeds are fresh in GCS.
 *
 * Expects a previous HTTP stage that called /v1/data/check-freshness.
 * HTTP stage output shape: { status, body: { feeds: [{ feed, status, age_hours, stale }] }, truncated }
 *
 * Params:
 *   freshnessStageIndex (number, default 0) — position of the HTTP stage
 *   maxAgeHours (number, default 7) — threshold for staleness
 *
 * Skips remaining stages if all feeds are fresh.
 */
export function archiveFreshness(input: CodeInput): CodeOutput {
  const stageIdx = (input.params?.freshnessStageIndex as number) ?? 0;
  const maxAge = (input.params?.maxAgeHours as number) ?? 7;

  const stage = input.stages[stageIdx] as { output?: string } | undefined;
  if (!stage?.output) {
    return { result: { error: "No freshness stage output found" }, skip: false };
  }

  let parsed: { body?: { feeds?: Array<{ feed: string; status: string; age_hours: number; stale: boolean }> } };
  try {
    parsed = JSON.parse(stage.output);
  } catch {
    return { result: { error: "Failed to parse freshness output" }, skip: false };
  }

  const feeds = parsed?.body?.feeds;
  if (!feeds || !Array.isArray(feeds)) {
    return { result: { error: "No feeds array in freshness response" }, skip: false };
  }

  const staleFeeds = feeds.filter((f) => f.age_hours > maxAge);
  const allFresh = staleFeeds.length === 0;

  return {
    result: {
      allFresh,
      feedCount: feeds.length,
      staleFeeds: staleFeeds.map((f) => ({
        feed: f.feed,
        age_hours: f.age_hours,
        status: f.status,
      })),
      summary: allFresh
        ? `All ${feeds.length} feeds fresh (max age ${maxAge}h)`
        : `${staleFeeds.length}/${feeds.length} feeds stale: ${staleFeeds.map((f) => `${f.feed} (${f.age_hours.toFixed(1)}h)`).join(", ")}`,
    },
    skip: allFresh,
  };
}
