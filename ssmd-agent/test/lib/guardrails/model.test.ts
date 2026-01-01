import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { checkModelAllowed } from "../../../src/lib/guardrails/mod.ts";

Deno.test("checkModelAllowed allows anthropic/claude-sonnet-4.5", () => {
  const result = checkModelAllowed("anthropic/claude-sonnet-4.5");
  assertEquals(result.allowed, true);
  assertEquals(result.reason, undefined);
});

Deno.test("checkModelAllowed allows qwen/qwen3-embedding-8b", () => {
  const result = checkModelAllowed("qwen/qwen3-embedding-8b");
  assertEquals(result.allowed, true);
  assertEquals(result.reason, undefined);
});

Deno.test("checkModelAllowed blocks openai/gpt-4o", () => {
  const result = checkModelAllowed("openai/gpt-4o");
  assertEquals(result.allowed, false);
  assertEquals(result.reason?.includes("not in the allowlist"), true);
});

Deno.test("checkModelAllowed blocks anthropic/claude-opus-4", () => {
  const result = checkModelAllowed("anthropic/claude-opus-4");
  assertEquals(result.allowed, false);
  assertEquals(result.reason?.includes("not in the allowlist"), true);
});
