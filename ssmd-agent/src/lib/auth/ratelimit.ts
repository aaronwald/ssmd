import { getRedis } from "../redis/mod.ts";

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

/**
 * Track token usage for a key prefix.
 * Increments prompt tokens, completion tokens, and request count.
 */
export async function trackTokenUsage(
  keyPrefix: string,
  usage: { promptTokens: number; completionTokens: number }
): Promise<void> {
  try {
    const redis = await getRedis();
    const pipeline = redis.pipeline();
    pipeline.incrby(`tokens:${keyPrefix}:prompt`, usage.promptTokens);
    pipeline.incrby(`tokens:${keyPrefix}:completion`, usage.completionTokens);
    pipeline.incr(`tokens:${keyPrefix}:requests`);
    await pipeline.flush();
  } catch (error) {
    console.error("Token usage tracking failed:", error);
    throw error;
  }
}

/**
 * Get token usage stats for a key prefix.
 * Returns total prompt tokens, completion tokens, and request count.
 */
export async function getTokenUsage(keyPrefix: string): Promise<{
  totalPromptTokens: number;
  totalCompletionTokens: number;
  totalLlmRequests: number;
}> {
  try {
    const redis = await getRedis();
    const [prompt, completion, requests] = await Promise.all([
      redis.get(`tokens:${keyPrefix}:prompt`),
      redis.get(`tokens:${keyPrefix}:completion`),
      redis.get(`tokens:${keyPrefix}:requests`),
    ]);
    return {
      totalPromptTokens: parseInt(prompt ?? "0", 10),
      totalCompletionTokens: parseInt(completion ?? "0", 10),
      totalLlmRequests: parseInt(requests ?? "0", 10),
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
