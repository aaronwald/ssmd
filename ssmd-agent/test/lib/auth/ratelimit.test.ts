import { assertEquals, assertExists } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  getDailyUsage,
  getModelUsage,
  getRateLimitForTier,
  getTokenUsage,
  RATE_LIMITS,
  trackTokenUsage,
} from "../../../src/lib/auth/ratelimit.ts";
import { getRedis } from "../../../src/lib/redis/mod.ts";

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

  // Cleanup test keys
  const redis = await getRedis();
  await redis.del(
    `tokens:${testPrefix}:prompt`,
    `tokens:${testPrefix}:completion`,
    `tokens:${testPrefix}:requests`
  );
});

Deno.test("trackTokenUsage with model creates daily bucket keys", async () => {
  const redisUrl = Deno.env.get("REDIS_URL");
  if (!redisUrl) {
    console.log("Skipping Redis test - REDIS_URL not set");
    return;
  }

  const testPrefix = `test_daily_${Date.now()}`;
  const testModel = "anthropic/claude-test";
  const today = new Date().toISOString().split("T")[0];

  await trackTokenUsage(testPrefix, { promptTokens: 100, completionTokens: 50 }, testModel);

  const redis = await getRedis();

  // Check daily bucket keys exist
  const baseKey = `tokens:${testPrefix}:daily:${today}:models:${testModel}`;
  const prompt = await redis.get(`${baseKey}:prompt`);
  const completion = await redis.get(`${baseKey}:completion`);
  const requests = await redis.get(`${baseKey}:requests`);

  assertEquals(parseInt(prompt ?? "0"), 100);
  assertEquals(parseInt(completion ?? "0"), 50);
  assertEquals(parseInt(requests ?? "0"), 1);

  // Cleanup
  await redis.del(
    `tokens:${testPrefix}:prompt`,
    `tokens:${testPrefix}:completion`,
    `tokens:${testPrefix}:requests`,
    `${baseKey}:prompt`,
    `${baseKey}:completion`,
    `${baseKey}:requests`
  );
});

Deno.test("getModelUsage aggregates across models", async () => {
  const redisUrl = Deno.env.get("REDIS_URL");
  if (!redisUrl) {
    console.log("Skipping Redis test - REDIS_URL not set");
    return;
  }

  const testPrefix = `test_model_usage_${Date.now()}`;
  const model1 = "anthropic/claude-1";
  const model2 = "openai/gpt-4";

  // Track usage for two different models
  await trackTokenUsage(testPrefix, { promptTokens: 100, completionTokens: 50 }, model1);
  await trackTokenUsage(testPrefix, { promptTokens: 200, completionTokens: 100 }, model2);
  await trackTokenUsage(testPrefix, { promptTokens: 50, completionTokens: 25 }, model1);

  const modelUsage = await getModelUsage(testPrefix);

  // Should have entries for both models
  assertEquals(modelUsage.length, 2);

  const claude = modelUsage.find((m) => m.model === model1);
  const gpt = modelUsage.find((m) => m.model === model2);

  assertExists(claude);
  assertExists(gpt);

  // Check aggregated values
  assertEquals(claude.promptTokens, 150); // 100 + 50
  assertEquals(claude.completionTokens, 75); // 50 + 25
  assertEquals(claude.requests, 2);

  assertEquals(gpt.promptTokens, 200);
  assertEquals(gpt.completionTokens, 100);
  assertEquals(gpt.requests, 1);

  // Cleanup
  const redis = await getRedis();
  const today = new Date().toISOString().split("T")[0];
  await redis.del(
    `tokens:${testPrefix}:prompt`,
    `tokens:${testPrefix}:completion`,
    `tokens:${testPrefix}:requests`,
    `tokens:${testPrefix}:daily:${today}:models:${model1}:prompt`,
    `tokens:${testPrefix}:daily:${today}:models:${model1}:completion`,
    `tokens:${testPrefix}:daily:${today}:models:${model1}:requests`,
    `tokens:${testPrefix}:daily:${today}:models:${model2}:prompt`,
    `tokens:${testPrefix}:daily:${today}:models:${model2}:completion`,
    `tokens:${testPrefix}:daily:${today}:models:${model2}:requests`
  );
});

Deno.test("getDailyUsage returns daily aggregates", async () => {
  const redisUrl = Deno.env.get("REDIS_URL");
  if (!redisUrl) {
    console.log("Skipping Redis test - REDIS_URL not set");
    return;
  }

  const testPrefix = `test_daily_usage_${Date.now()}`;
  const testModel = "anthropic/claude-daily";

  // Track some usage
  await trackTokenUsage(testPrefix, { promptTokens: 100, completionTokens: 50 }, testModel);
  await trackTokenUsage(testPrefix, { promptTokens: 200, completionTokens: 100 }, testModel);

  const dailyUsage = await getDailyUsage(testPrefix, 7);

  // Should have at least today's usage
  assertEquals(dailyUsage.length >= 1, true);

  const today = new Date().toISOString().split("T")[0];
  const todayUsage = dailyUsage.find((d) => d.date === today);

  assertExists(todayUsage);
  assertEquals(todayUsage.promptTokens, 300);
  assertEquals(todayUsage.completionTokens, 150);
  assertEquals(todayUsage.requests, 2);

  // Cleanup
  const redis = await getRedis();
  await redis.del(
    `tokens:${testPrefix}:prompt`,
    `tokens:${testPrefix}:completion`,
    `tokens:${testPrefix}:requests`,
    `tokens:${testPrefix}:daily:${today}:models:${testModel}:prompt`,
    `tokens:${testPrefix}:daily:${today}:models:${testModel}:completion`,
    `tokens:${testPrefix}:daily:${today}:models:${testModel}:requests`
  );
});

Deno.test("getTokenUsage includes modelUsage and dailyUsage", async () => {
  const redisUrl = Deno.env.get("REDIS_URL");
  if (!redisUrl) {
    console.log("Skipping Redis test - REDIS_URL not set");
    return;
  }

  const testPrefix = `test_full_usage_${Date.now()}`;
  const testModel = "anthropic/claude-full";

  await trackTokenUsage(testPrefix, { promptTokens: 100, completionTokens: 50 }, testModel);

  const usage = await getTokenUsage(testPrefix);

  // Check cumulative totals
  assertEquals(usage.totalPromptTokens, 100);
  assertEquals(usage.totalCompletionTokens, 50);
  assertEquals(usage.totalLlmRequests, 1);

  // Check model usage is included
  assertEquals(usage.modelUsage.length, 1);
  assertEquals(usage.modelUsage[0].model, testModel);

  // Check daily usage is included
  assertEquals(usage.dailyUsage.length >= 1, true);

  // Cleanup
  const redis = await getRedis();
  const today = new Date().toISOString().split("T")[0];
  await redis.del(
    `tokens:${testPrefix}:prompt`,
    `tokens:${testPrefix}:completion`,
    `tokens:${testPrefix}:requests`,
    `tokens:${testPrefix}:daily:${today}:models:${testModel}:prompt`,
    `tokens:${testPrefix}:daily:${today}:models:${testModel}:completion`,
    `tokens:${testPrefix}:daily:${today}:models:${testModel}:requests`
  );
});
