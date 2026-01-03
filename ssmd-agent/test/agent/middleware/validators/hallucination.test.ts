import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { detectHallucination } from "../../../../src/agent/middleware/validators/hallucination.ts";

Deno.test("detectHallucination - detects price claims without data", () => {
  const result = detectHallucination("The current price is $45.50 for this market.");
  assertEquals(result.detected, true);
  assertEquals(result.pattern !== undefined, true);
});

Deno.test("detectHallucination - detects market count claims", () => {
  const result = detectHallucination("There are 150 active markets right now.");
  assertEquals(result.detected, true);
});

Deno.test("detectHallucination - detects overconfident predictions", () => {
  const result = detectHallucination("This will definitely happen tomorrow.");
  assertEquals(result.detected, true);
});

Deno.test("detectHallucination - detects guaranteed claims", () => {
  const result = detectHallucination("You are 100% certain to win.");
  assertEquals(result.detected, true);
});

Deno.test("detectHallucination - allows normal responses", () => {
  const result = detectHallucination("I can help you look up market data.");
  assertEquals(result.detected, false);
});

Deno.test("detectHallucination - allows hedged language", () => {
  const result = detectHallucination("Based on historical data, this might be likely.");
  assertEquals(result.detected, false);
});
