import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { changelogDiff } from "./changelog-diff.ts";
import type { CodeInput } from "./mod.ts";

function makeInput(stage0Output: unknown): CodeInput {
  return {
    stages: {
      0: { output: JSON.stringify(stage0Output) },
    },
    triggerInfo: {},
    date: "2026-03-12",
  };
}

Deno.test("changelogDiff: skip when changelog unchanged", () => {
  const input = makeInput({ body: { changed: false } });
  const result = changelogDiff(input);
  assertEquals(result.skip, true);
  assertEquals((result.result as Record<string, unknown>).skipped, true);
  assertEquals((result.result as Record<string, unknown>).reason, "Changelog unchanged");
});

Deno.test("changelogDiff: no skip when changelog changed", () => {
  const input = makeInput({ body: { changed: true } });
  const result = changelogDiff(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).changed, true);
});

Deno.test("changelogDiff: no skip when changed is undefined", () => {
  const input = makeInput({ body: {} });
  const result = changelogDiff(input);
  assertEquals(result.skip, false);
});

Deno.test("changelogDiff: error when no stage 0 output", () => {
  const input: CodeInput = { stages: {}, triggerInfo: {}, date: "2026-03-12" };
  const result = changelogDiff(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "No stage 0 output found");
});

Deno.test("changelogDiff: error on invalid JSON", () => {
  const input: CodeInput = {
    stages: { 0: { output: "not-json{{{" } },
    triggerInfo: {},
    date: "2026-03-12",
  };
  const result = changelogDiff(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "Failed to parse stage 0 output");
});
