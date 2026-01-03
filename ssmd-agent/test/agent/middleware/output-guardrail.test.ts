import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { applyOutputGuardrail, type GuardrailResult } from "../../../src/agent/middleware/output-guardrail.ts";

Deno.test("applyOutputGuardrail - blocks toxic content", () => {
  const result = applyOutputGuardrail("You idiot, that's wrong!");
  assertEquals(result.allowed, false);
  assertEquals(result.reason?.includes("toxic"), true);
});

Deno.test("applyOutputGuardrail - blocks hallucinations", () => {
  const result = applyOutputGuardrail("The current price is $45.50.");
  assertEquals(result.allowed, false);
  assertEquals(result.reason?.includes("hallucination"), true);
});

Deno.test("applyOutputGuardrail - allows clean responses", () => {
  const result = applyOutputGuardrail("I can help you look up market information.");
  assertEquals(result.allowed, true);
});

Deno.test("applyOutputGuardrail - returns modified content when allowed", () => {
  const input = "Here's what I found in the data.";
  const result = applyOutputGuardrail(input);
  assertEquals(result.allowed, true);
  assertEquals(result.content, input);
});

Deno.test("applyOutputGuardrail - respects disabled toxicity check", () => {
  const result = applyOutputGuardrail("You idiot!", { toxicityEnabled: false });
  assertEquals(result.allowed, true);
});

Deno.test("applyOutputGuardrail - respects disabled hallucination check", () => {
  const result = applyOutputGuardrail("The current price is $50.", { hallucinationEnabled: false });
  assertEquals(result.allowed, true);
});
