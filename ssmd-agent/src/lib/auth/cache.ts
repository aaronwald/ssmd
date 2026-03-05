import { getRedis } from "../redis/mod.ts";

const CACHE_TTL = 300; // 5 minutes

export interface CachedKeyInfo {
  userId: string;
  userEmail: string;
  scopes: string[];
  rateLimitTier: string;
  revoked: boolean;
  expiresAt: string | null;
  allowedFeeds: string[];
  dateRangeStart: string;  // YYYY-MM-DD
  dateRangeEnd: string;    // YYYY-MM-DD
  billable: boolean;
  disabledAt: string | null;
}

/**
 * Build a cache key from prefix + secret hash.
 * The cache key itself proves the secret was verified — no hash stored in value.
 */
export function buildCacheKey(prefix: string, secretHash: string): string {
  return `apikey:${prefix}:${secretHash}`;
}

/**
 * Get verified key info from Redis cache.
 */
export async function getCachedKeyInfo(cacheKey: string): Promise<CachedKeyInfo | null> {
  const redis = await getRedis();
  const cached = await redis.get(cacheKey);

  if (!cached) {
    return null;
  }

  return JSON.parse(cached) as CachedKeyInfo;
}

/**
 * Cache verified key info in Redis.
 */
export async function cacheKeyInfo(cacheKey: string, info: CachedKeyInfo): Promise<void> {
  const redis = await getRedis();
  await redis.setex(cacheKey, CACHE_TTL, JSON.stringify(info));
}

/**
 * Invalidate cached key info by prefix.
 * Scans for all cache keys matching this prefix since the full cache key
 * includes the secret hash (which we don't have during invalidation).
 */
export async function invalidateKeyCache(prefix: string): Promise<void> {
  const redis = await getRedis();
  let cursor = 0;
  do {
    const result = await redis.scan(cursor, { pattern: `apikey:${prefix}:*`, count: 100 });
    cursor = Number(result[0]);
    const keys = result[1] as string[];
    if (keys.length > 0) {
      await redis.del(...keys);
    }
  } while (cursor !== 0);
}
