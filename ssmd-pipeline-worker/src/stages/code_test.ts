import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { executeCode } from "./code.ts";
import type { ExecuteContext } from "./mod.ts";
import type { StageConfig } from "../types.ts";
import { codeFunctions } from "../functions/mod.ts";

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
  // Register a stub function so we reach the _context check
  codeFunctions["test-stub"] = () => ({ result: null });
  try {
    const config: StageConfig = { function: "test-stub" };
    const result = await executeCode(config, stubCtx, signal);
    assertEquals(result.status, "failed");
    assertEquals(result.error, "Code stage requires '_context' injected by worker");
  } finally {
    delete codeFunctions["test-stub"];
  }
});
