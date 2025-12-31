import { getRedis } from "../redis/mod.ts";

const CACHE_TTL = 300; // 5 minutes

export interface CachedKeyInfo {
  keyHash: string;
  userId: string;
  userEmail: string;
  scopes: string[];
  rateLimitTier: string;
  revoked: boolean;
}

/**
 * Get key info from Redis cache.
 */
export async function getCachedKeyInfo(prefix: string): Promise<CachedKeyInfo | null> {
  const redis = await getRedis();
  const cached = await redis.get(`apikey:${prefix}`);

  if (!cached) {
    return null;
  }

  return JSON.parse(cached) as CachedKeyInfo;
}

/**
 * Cache key info in Redis.
 */
export async function cacheKeyInfo(prefix: string, info: CachedKeyInfo): Promise<void> {
  const redis = await getRedis();
  await redis.setex(`apikey:${prefix}`, CACHE_TTL, JSON.stringify(info));
}

/**
 * Invalidate cached key info.
 */
export async function invalidateKeyCache(prefix: string): Promise<void> {
  const redis = await getRedis();
  await redis.del(`apikey:${prefix}`);
}
