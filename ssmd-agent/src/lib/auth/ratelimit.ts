import { getRedis } from "../redis/mod.ts";
import { calculateCost } from "../pricing/mod.ts";

export const RATE_LIMITS = {
  standard: 120,  // requests per minute
  elevated: 1200, // requests per minute
} as const;

const WINDOW_SECONDS = 60;

/**
 * Get rate limit for a tier.
 */
export function getRateLimitForTier(tier: string): number {
  return RATE_LIMITS[tier as keyof typeof RATE_LIMITS] ?? RATE_LIMITS.standard;
}

/**
 * Check if request is within rate limit.
 * Uses Redis sorted sets for sliding window.
 * Returns { allowed: boolean, remaining: number, resetAt: number }
 */
export async function checkRateLimit(
  keyPrefix: string,
  tier: string
): Promise<{ allowed: boolean; remaining: number; resetAt: number }> {
  try {
    const redis = await getRedis();
    const maxRequests = getRateLimitForTier(tier);
    const now = Date.now();
    const windowStart = now - WINDOW_SECONDS * 1000;
    const key = `ratelimit:${keyPrefix}`;

    // Use pipeline for atomic operations
    const pipeline = redis.pipeline();
    pipeline.zremrangebyscore(key, 0, windowStart); // Remove old entries
    pipeline.zadd(key, { [now.toString()]: now });  // Add current request
    pipeline.zcard(key);                             // Count requests in window
    pipeline.expire(key, WINDOW_SECONDS);            // Set TTL
    const results = await pipeline.flush();

    const requestCount = results[2] as number;
    const allowed = requestCount <= maxRequests;
    const remaining = Math.max(0, maxRequests - requestCount);
    const resetAt = now + WINDOW_SECONDS * 1000;

    return { allowed, remaining, resetAt };
  } catch (error) {
    console.error("Rate limit check failed:", error);
    throw error;
  }
}

/**
 * Increment rate limit hit counter (for metrics).
 */
export async function incrementRateLimitHits(keyPrefix: string): Promise<void> {
  const redis = await getRedis();
  await redis.incr(`ratelimit_hits:${keyPrefix}`);
}

const DAILY_BUCKET_TTL_SECONDS = 90 * 24 * 60 * 60; // 90 days

/**
 * Get current date in YYYY-MM-DD format.
 */
function getDateKey(): string {
  return new Date().toISOString().split("T")[0];
}

/**
 * Track token usage for a key prefix.
 * Increments prompt tokens, completion tokens, and request count.
 * Optionally tracks per-model daily usage.
 */
export async function trackTokenUsage(
  keyPrefix: string,
  usage: { promptTokens: number; completionTokens: number },
  model?: string
): Promise<void> {
  try {
    const redis = await getRedis();
    const pipeline = redis.pipeline();

    // Cumulative totals (backward compatible)
    pipeline.incrby(`tokens:${keyPrefix}:prompt`, usage.promptTokens);
    pipeline.incrby(`tokens:${keyPrefix}:completion`, usage.completionTokens);
    pipeline.incr(`tokens:${keyPrefix}:requests`);

    // Per-model daily tracking (new)
    if (model) {
      const date = getDateKey();
      const baseKey = `tokens:${keyPrefix}:daily:${date}:models:${model}`;

      pipeline.incrby(`${baseKey}:prompt`, usage.promptTokens);
      pipeline.incrby(`${baseKey}:completion`, usage.completionTokens);
      pipeline.incr(`${baseKey}:requests`);

      // Set TTL on all keys
      pipeline.expire(`${baseKey}:prompt`, DAILY_BUCKET_TTL_SECONDS);
      pipeline.expire(`${baseKey}:completion`, DAILY_BUCKET_TTL_SECONDS);
      pipeline.expire(`${baseKey}:requests`, DAILY_BUCKET_TTL_SECONDS);
    }

    await pipeline.flush();
  } catch (error) {
    console.error("Token usage tracking failed:", error);
    throw error;
  }
}

export interface ModelUsage {
  model: string;
  promptTokens: number;
  completionTokens: number;
  requests: number;
  costUsd: number;
}

export interface DailyUsage {
  date: string;
  promptTokens: number;
  completionTokens: number;
  requests: number;
  costUsd: number;
}

/**
 * Get per-model usage for a key prefix.
 * Aggregates across all daily buckets within the date range.
 */
export async function getModelUsage(
  keyPrefix: string,
  startDate?: string,
  endDate?: string
): Promise<ModelUsage[]> {
  try {
    const redis = await getRedis();

    // Scan for all daily model keys for this prefix
    const pattern = `tokens:${keyPrefix}:daily:*:models:*:requests`;
    const modelKeys: string[] = [];

    let cursor = 0;
    do {
      const [nextCursor, keys] = await redis.scan(cursor, {
        pattern: pattern,
        count: 100,
      });
      cursor = parseInt(nextCursor);
      modelKeys.push(...keys);
    } while (cursor !== 0);

    // Parse keys to extract date and model, aggregate by model
    const modelAggregates: Record<string, { prompt: number; completion: number; requests: number }> = {};

    for (const key of modelKeys) {
      // Key format: tokens:{keyPrefix}:daily:{date}:models:{model}:requests
      const match = key.match(/^tokens:[^:]+:daily:(\d{4}-\d{2}-\d{2}):models:(.+):requests$/);
      if (!match) continue;

      const [, date, model] = match;

      // Filter by date range if provided
      if (startDate && date < startDate) continue;
      if (endDate && date > endDate) continue;

      // Get values for this date/model combination
      const baseKey = `tokens:${keyPrefix}:daily:${date}:models:${model}`;
      const [prompt, completion, requests] = await Promise.all([
        redis.get(`${baseKey}:prompt`),
        redis.get(`${baseKey}:completion`),
        redis.get(`${baseKey}:requests`),
      ]);

      if (!modelAggregates[model]) {
        modelAggregates[model] = { prompt: 0, completion: 0, requests: 0 };
      }

      modelAggregates[model].prompt += parseInt(prompt ?? "0", 10);
      modelAggregates[model].completion += parseInt(completion ?? "0", 10);
      modelAggregates[model].requests += parseInt(requests ?? "0", 10);
    }

    // Convert to array with cost calculation
    const usage: ModelUsage[] = [];
    for (const [model, agg] of Object.entries(modelAggregates)) {
      const costUsd = await calculateCost(model, agg.prompt, agg.completion);
      usage.push({
        model,
        promptTokens: agg.prompt,
        completionTokens: agg.completion,
        requests: agg.requests,
        costUsd,
      });
    }

    // Sort by cost descending
    usage.sort((a, b) => b.costUsd - a.costUsd);

    return usage;
  } catch (error) {
    console.error("Get model usage failed:", error);
    return [];
  }
}

/**
 * Get daily usage breakdown for a key prefix.
 * Returns usage for each day in the specified range.
 */
export async function getDailyUsage(
  keyPrefix: string,
  days: number = 30
): Promise<DailyUsage[]> {
  try {
    const redis = await getRedis();

    // Generate date keys for the last N days
    const dates: string[] = [];
    const today = new Date();
    for (let i = 0; i < days; i++) {
      const date = new Date(today);
      date.setDate(date.getDate() - i);
      dates.push(date.toISOString().split("T")[0]);
    }

    // For each date, scan for model keys and aggregate
    const dailyUsage: DailyUsage[] = [];

    for (const date of dates) {
      const pattern = `tokens:${keyPrefix}:daily:${date}:models:*:requests`;
      const modelKeys: string[] = [];

      let cursor = 0;
      do {
        const [nextCursor, keys] = await redis.scan(cursor, {
          pattern: pattern,
          count: 100,
        });
        cursor = parseInt(nextCursor);
        modelKeys.push(...keys);
      } while (cursor !== 0);

      if (modelKeys.length === 0) continue;

      // Aggregate all models for this date
      let totalPrompt = 0;
      let totalCompletion = 0;
      let totalRequests = 0;
      let totalCost = 0;

      for (const key of modelKeys) {
        const match = key.match(/^tokens:[^:]+:daily:\d{4}-\d{2}-\d{2}:models:(.+):requests$/);
        if (!match) continue;

        const model = match[1];
        const baseKey = `tokens:${keyPrefix}:daily:${date}:models:${model}`;

        const [prompt, completion, requests] = await Promise.all([
          redis.get(`${baseKey}:prompt`),
          redis.get(`${baseKey}:completion`),
          redis.get(`${baseKey}:requests`),
        ]);

        const p = parseInt(prompt ?? "0", 10);
        const c = parseInt(completion ?? "0", 10);
        const r = parseInt(requests ?? "0", 10);

        totalPrompt += p;
        totalCompletion += c;
        totalRequests += r;
        totalCost += await calculateCost(model, p, c);
      }

      dailyUsage.push({
        date,
        promptTokens: totalPrompt,
        completionTokens: totalCompletion,
        requests: totalRequests,
        costUsd: Math.round(totalCost * 1_000_000) / 1_000_000,
      });
    }

    // Sort by date ascending (oldest first)
    dailyUsage.sort((a, b) => a.date.localeCompare(b.date));

    return dailyUsage;
  } catch (error) {
    console.error("Get daily usage failed:", error);
    return [];
  }
}

/**
 * Get token usage stats for a key prefix.
 * Returns total prompt tokens, completion tokens, request count,
 * model breakdown, daily breakdown, and estimated cost.
 */
export async function getTokenUsage(keyPrefix: string): Promise<{
  totalPromptTokens: number;
  totalCompletionTokens: number;
  totalLlmRequests: number;
  modelUsage: ModelUsage[];
  dailyUsage: DailyUsage[];
  totalCostUsd: number;
}> {
  try {
    const redis = await getRedis();
    const [prompt, completion, requests] = await Promise.all([
      redis.get(`tokens:${keyPrefix}:prompt`),
      redis.get(`tokens:${keyPrefix}:completion`),
      redis.get(`tokens:${keyPrefix}:requests`),
    ]);

    // Get per-model and daily breakdowns
    const [modelUsage, dailyUsage] = await Promise.all([
      getModelUsage(keyPrefix),
      getDailyUsage(keyPrefix, 30),
    ]);

    // Calculate total cost from model usage
    const totalCostUsd = modelUsage.reduce((sum, m) => sum + m.costUsd, 0);

    return {
      totalPromptTokens: parseInt(prompt ?? "0", 10),
      totalCompletionTokens: parseInt(completion ?? "0", 10),
      totalLlmRequests: parseInt(requests ?? "0", 10),
      modelUsage,
      dailyUsage,
      totalCostUsd: Math.round(totalCostUsd * 1_000_000) / 1_000_000,
    };
  } catch (error) {
    console.error("Get token usage failed:", error);
    throw error;
  }
}

/**
 * Get usage stats for a key prefix.
 * Returns current request count in window and total rate limit hits.
 */
export async function getUsageForPrefix(
  keyPrefix: string,
  tier: string
): Promise<{
  keyPrefix: string;
  requestsInWindow: number;
  rateLimitHits: number;
  windowSeconds: number;
  limit: number;
  tier: string;
}> {
  const redis = await getRedis();
  const now = Date.now();
  const windowStart = now - WINDOW_SECONDS * 1000;
  const key = `ratelimit:${keyPrefix}`;
  const hitsKey = `ratelimit_hits:${keyPrefix}`;

  // Clean old entries and get count
  await redis.zremrangebyscore(key, 0, windowStart);
  const requestsInWindow = await redis.zcard(key) ?? 0;
  const rateLimitHits = parseInt(await redis.get(hitsKey) ?? "0", 10);

  return {
    keyPrefix,
    requestsInWindow,
    rateLimitHits,
    windowSeconds: WINDOW_SECONDS,
    limit: getRateLimitForTier(tier),
    tier,
  };
}
