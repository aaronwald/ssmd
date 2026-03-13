import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { archiveFreshness } from "./archive-freshness.ts";
import type { CodeInput } from "./mod.ts";

function makeInput(body: unknown, params?: Record<string, unknown>): CodeInput {
  return {
    stages: {
      0: { output: JSON.stringify({ body }) },
    },
    triggerInfo: {},
    date: "2026-03-13",
    params,
  };
}

Deno.test("archive-freshness: all feeds fresh → skip", () => {
  const result = archiveFreshness(makeInput({
    feeds: [
      { feed: "kalshi", status: "fresh", age_hours: 2.5, stale: false },
      { feed: "kraken-futures", status: "fresh", age_hours: 3.0, stale: false },
      { feed: "kraken-spot", status: "fresh", age_hours: 2.8, stale: false },
      { feed: "kalshi-sports", status: "fresh", age_hours: 2.5, stale: false },
    ],
  }));
  assertEquals(result.skip, true);
  assertEquals((result.result as Record<string, unknown>).allFresh, true);
  assertEquals((result.result as Record<string, unknown>).feedCount, 4);
});

Deno.test("archive-freshness: stale feed → no skip", () => {
  const result = archiveFreshness(makeInput({
    feeds: [
      { feed: "kalshi", status: "fresh", age_hours: 2.5, stale: false },
      { feed: "kraken-spot", status: "stale", age_hours: 12.0, stale: true },
    ],
  }));
  assertEquals(result.skip, false);
  const r = result.result as Record<string, unknown>;
  assertEquals(r.allFresh, false);
  assertEquals((r.staleFeeds as unknown[]).length, 1);
});

Deno.test("archive-freshness: custom maxAgeHours threshold", () => {
  const result = archiveFreshness(makeInput(
    {
      feeds: [
        { feed: "kalshi", status: "fresh", age_hours: 5.0, stale: false },
      ],
    },
    { maxAgeHours: 4 },
  ));
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).allFresh, false);
});

Deno.test("archive-freshness: no stage output → error", () => {
  const input: CodeInput = { stages: {}, triggerInfo: {}, date: "2026-03-13" };
  const result = archiveFreshness(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "No freshness stage output found");
});

Deno.test("archive-freshness: missing feeds array → error", () => {
  const result = archiveFreshness(makeInput({ noFeeds: true }));
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "No feeds array in freshness response");
});
