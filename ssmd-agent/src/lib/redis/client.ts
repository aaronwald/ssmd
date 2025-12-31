import { connect, type Redis } from "https://deno.land/x/redis@v0.32.4/mod.ts";

let redisClient: Redis | null = null;

/**
 * Get or create Redis connection.
 */
export async function getRedis(): Promise<Redis> {
  if (redisClient) {
    return redisClient;
  }

  const redisUrl = Deno.env.get("REDIS_URL") ?? "redis://localhost:6379";
  const url = new URL(redisUrl);

  redisClient = await connect({
    hostname: url.hostname,
    port: parseInt(url.port || "6379"),
  });

  return redisClient;
}

/**
 * Close Redis connection.
 */
export function closeRedis(): void {
  if (redisClient) {
    redisClient.close();
    redisClient = null;
  }
}
