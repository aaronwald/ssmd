import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  getRateLimitForTier,
  getTokenUsage,
  RATE_LIMITS,
  trackTokenUsage,
} from "../../../src/lib/auth/ratelimit.ts";

Deno.test("getRateLimitForTier returns correct limits", () => {
  assertEquals(getRateLimitForTier("standard"), RATE_LIMITS.standard);
  assertEquals(getRateLimitForTier("elevated"), RATE_LIMITS.elevated);
  assertEquals(getRateLimitForTier("unknown"), RATE_LIMITS.standard); // fallback
});

Deno.test("trackTokenUsage and getTokenUsage work together", async () => {
  const redisUrl = Deno.env.get("REDIS_URL");
  if (!redisUrl) {
    console.log("Skipping Redis test - REDIS_URL not set");
    return;
  }

  const testPrefix = `test_${Date.now()}`;
  await trackTokenUsage(testPrefix, { promptTokens: 100, completionTokens: 50 });
  await trackTokenUsage(testPrefix, { promptTokens: 200, completionTokens: 100 });

  const usage = await getTokenUsage(testPrefix);
  assertEquals(usage.totalPromptTokens, 300);
  assertEquals(usage.totalCompletionTokens, 150);
  assertEquals(usage.totalLlmRequests, 2);
});
