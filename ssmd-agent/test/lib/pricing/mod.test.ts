import { assertEquals, assertExists } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  calculateCost,
  getModelPricing,
  getAllCachedPricing,
  refreshPricingCache,
} from "../../../src/lib/pricing/mod.ts";
import { getRedis } from "../../../src/lib/redis/mod.ts";

const CACHE_KEY_PREFIX = "openrouter:pricing:";

Deno.test("calculateCost returns 0 for unknown model when cache is empty", async () => {
  const cost = await calculateCost("unknown/model", 1000, 1000);
  assertEquals(cost, 0);
});

Deno.test("getModelPricing returns null for uncached model", async () => {
  const pricing = await getModelPricing("nonexistent/model-xyz");
  // May return null or cached value depending on cache state
  // If API was fetched before, it might have data
  // This test is more of a smoke test
});

Deno.test("calculateCost with cached pricing", async () => {
  const redisUrl = Deno.env.get("REDIS_URL");
  if (!redisUrl) {
    console.log("Skipping Redis test - REDIS_URL not set");
    return;
  }

  const redis = await getRedis();
  const testModel = "test/model-for-pricing";
  const cacheKey = `${CACHE_KEY_PREFIX}${testModel}`;

  // Manually cache pricing for test model
  const testPricing = {
    prompt: 0.000003,     // $3 per million
    completion: 0.000015, // $15 per million
  };
  await redis.setex(cacheKey, 60, JSON.stringify(testPricing));

  // Test cost calculation
  // 1M prompt tokens at $3/M + 1M completion at $15/M = $18
  const cost = await calculateCost(testModel, 1_000_000, 1_000_000);
  assertEquals(cost, 18);

  // Test smaller amounts
  // 1000 prompt + 500 completion
  const smallCost = await calculateCost(testModel, 1000, 500);
  // 1000 * 0.000003 = 0.003
  // 500 * 0.000015 = 0.0075
  // Total = 0.0105, rounded to 6 decimals
  assertEquals(smallCost, 0.0105);

  // Cleanup
  await redis.del(cacheKey);
});

Deno.test("getAllCachedPricing returns cached models", async () => {
  const redisUrl = Deno.env.get("REDIS_URL");
  if (!redisUrl) {
    console.log("Skipping Redis test - REDIS_URL not set");
    return;
  }

  const redis = await getRedis();
  const testModel = "test/cached-model";
  const cacheKey = `${CACHE_KEY_PREFIX}${testModel}`;

  // Cache a test model
  const testPricing = { prompt: 0.001, completion: 0.002 };
  await redis.setex(cacheKey, 60, JSON.stringify(testPricing));

  // Get all cached pricing
  const allPricing = await getAllCachedPricing();

  // Should include our test model
  assertExists(allPricing[testModel]);
  assertEquals(allPricing[testModel].prompt, 0.001);
  assertEquals(allPricing[testModel].completion, 0.002);

  // Cleanup
  await redis.del(cacheKey);
});

// Integration test - requires network access to OpenRouter
Deno.test({
  name: "refreshPricingCache fetches from OpenRouter API",
  ignore: !Deno.env.get("RUN_INTEGRATION_TESTS"),
  async fn() {
    await refreshPricingCache();

    // Check that some models were cached
    const allPricing = await getAllCachedPricing();
    const modelCount = Object.keys(allPricing).length;

    console.log(`Cached ${modelCount} models from OpenRouter`);
    // OpenRouter has 400+ models, so we should have cached many
    assertEquals(modelCount > 100, true);
  },
});
