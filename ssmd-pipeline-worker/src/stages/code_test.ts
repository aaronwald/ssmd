import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { executeCode } from "./code.ts";
import type { ExecuteContext } from "./mod.ts";
import type { StageConfig } from "../types.ts";

const stubCtx: ExecuteContext = {
  readonlySql: null,
  dataTsUrl: "http://localhost:8081",
  adminApiKey: "test-key",
};

const signal = AbortSignal.timeout(5000);

Deno.test("executeCode: error when function name missing", async () => {
  const config: StageConfig = {};
  const result = await executeCode(config, stubCtx, signal);
  assertEquals(result.status, "failed");
  assertEquals(result.error, "Code stage requires 'function' in config");
});

Deno.test("executeCode: error when function not found", async () => {
  const config: StageConfig = { function: "nonexistent" };
  const result = await executeCode(config, stubCtx, signal);
  assertEquals(result.status, "failed");
  assertEquals(result.error?.startsWith("Unknown code function: nonexistent"), true);
});

Deno.test("executeCode: error when _context missing", async () => {
  const config: StageConfig = { function: "changelog-diff" };
  const result = await executeCode(config, stubCtx, signal);
  assertEquals(result.status, "failed");
  assertEquals(result.error, "Code stage requires '_context' injected by worker");
});

Deno.test("executeCode: runs changelog-diff successfully", async () => {
  const config: StageConfig = {
    function: "changelog-diff",
    _context: {
      stages: {
        0: { output: JSON.stringify({ body: { changed: true } }) },
      },
      triggerInfo: {},
      date: "2026-03-12",
    },
  };
  const result = await executeCode(config, stubCtx, signal);
  assertEquals(result.status, "completed");
  const output = result.output as { skip: boolean; result: { changed: boolean } };
  assertEquals(output.skip, false);
  assertEquals(output.result.changed, true);
});

Deno.test("executeCode: skip propagated from changelog-diff", async () => {
  const config: StageConfig = {
    function: "changelog-diff",
    _context: {
      stages: {
        0: { output: JSON.stringify({ body: { changed: false } }) },
      },
      triggerInfo: {},
      date: "2026-03-12",
    },
  };
  const result = await executeCode(config, stubCtx, signal);
  assertEquals(result.status, "completed");
  const output = result.output as { skip: boolean };
  assertEquals(output.skip, true);
});
