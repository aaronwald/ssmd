import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { getRateLimitForTier, RATE_LIMITS } from "../../../src/lib/auth/ratelimit.ts";

Deno.test("getRateLimitForTier returns correct limits", () => {
  assertEquals(getRateLimitForTier("standard"), RATE_LIMITS.standard);
  assertEquals(getRateLimitForTier("elevated"), RATE_LIMITS.elevated);
  assertEquals(getRateLimitForTier("unknown"), RATE_LIMITS.standard); // fallback
});
