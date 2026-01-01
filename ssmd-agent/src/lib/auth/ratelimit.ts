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
    // On error, allow the request to avoid blocking
    // return { allowed: false, remaining: 0, resetAt: Date.now() + WINDOW_SECONDS * 1000 };
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
