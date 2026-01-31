import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import type { Signal, SignalResult, ComposerDecision } from "../../src/momentum/signals/types.ts";

Deno.test("SignalResult has required fields", () => {
  const result: SignalResult = {
    name: "test",
    score: 0.5,
    confidence: 0.8,
    reason: "test reason",
  };
  assertEquals(result.name, "test");
  assertEquals(result.score, 0.5);
  assertEquals(result.confidence, 0.8);
});

Deno.test("ComposerDecision has required fields", () => {
  const decision: ComposerDecision = {
    enter: true,
    side: "yes",
    price: 55,
    score: 0.7,
    signals: [{ name: "test", score: 0.5, confidence: 0.8, reason: "r" }],
  };
  assertEquals(decision.enter, true);
  assertEquals(decision.side, "yes");
  assertEquals(decision.price, 55);
});
