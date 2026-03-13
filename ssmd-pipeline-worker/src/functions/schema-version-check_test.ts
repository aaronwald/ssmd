import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { schemaVersionCheck } from "./schema-version-check.ts";
import type { CodeInput } from "./mod.ts";

Deno.test("schemaVersionCheck: passes versions through from default stage index 3", () => {
  const versions = { kalshi: { ticker: 2, trade: 1 }, kraken: { ticker: 1 } };
  const input: CodeInput = {
    stages: {
      3: { output: JSON.stringify(versions) },
    },
    triggerInfo: {},
    date: "2026-03-12",
  };
  const result = schemaVersionCheck(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).versions, versions);
});

Deno.test("schemaVersionCheck: uses custom schemaStageIndex from params", () => {
  const versions = { kalshi: { ticker: 3 } };
  const input: CodeInput = {
    stages: {
      5: { output: JSON.stringify(versions) },
    },
    triggerInfo: {},
    date: "2026-03-12",
    params: { schemaStageIndex: 5 },
  };
  const result = schemaVersionCheck(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).versions, versions);
});

Deno.test("schemaVersionCheck: error when stage missing", () => {
  const input: CodeInput = { stages: {}, triggerInfo: {}, date: "2026-03-12" };
  const result = schemaVersionCheck(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "No schema versions stage output found");
});

Deno.test("schemaVersionCheck: error on invalid JSON", () => {
  const input: CodeInput = {
    stages: { 3: { output: "bad-json" } },
    triggerInfo: {},
    date: "2026-03-12",
  };
  const result = schemaVersionCheck(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "Failed to parse schema versions");
});
