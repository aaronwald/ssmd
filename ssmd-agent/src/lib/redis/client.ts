import { connect, type Redis } from "https://deno.land/x/redis@v0.32.4/mod.ts";

let redisClient: Redis | null = null;

function parseRedisUrl(): { hostname: string; port: number; password?: string } {
  const redisUrl = Deno.env.get("REDIS_URL") ?? "redis://localhost:6379";
  const url = new URL(redisUrl);
  const options: { hostname: string; port: number; password?: string } = {
    hostname: url.hostname,
    port: parseInt(url.port || "6379"),
  };
  if (url.password) {
    options.password = decodeURIComponent(url.password);
  }
  return options;
}

/**
 * Get or create Redis connection.
 * Tests connection health with PING; reconnects if stale.
 */
export async function getRedis(): Promise<Redis> {
  if (redisClient) {
    try {
      await redisClient.ping();
      return redisClient;
    } catch {
      console.warn("[redis] Connection stale, reconnecting...");
      try { redisClient.close(); } catch { /* ignore */ }
      redisClient = null;
    }
  }

  const options = parseRedisUrl();
  redisClient = await connect(options);
  console.log("[redis] Connected to", options.hostname);
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
