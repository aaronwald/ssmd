/**
 * API keys database operations
 */
import { eq, isNull, and } from "drizzle-orm";
import { apiKeys, type ApiKey, type NewApiKey } from "./schema.ts";
import type { Database } from "./client.ts";

/**
 * Get API key by prefix (for validation).
 */
export async function getApiKeyByPrefix(
  db: Database,
  prefix: string
): Promise<ApiKey | null> {
  const result = await db
    .select()
    .from(apiKeys)
    .where(and(eq(apiKeys.keyPrefix, prefix), isNull(apiKeys.revokedAt)))
    .limit(1);

  return result[0] ?? null;
}

/**
 * Create a new API key.
 */
export async function createApiKey(
  db: Database,
  key: NewApiKey
): Promise<ApiKey> {
  const result = await db.insert(apiKeys).values(key).returning();
  return result[0];
}

/**
 * List API keys for a user.
 */
export async function listApiKeysByUser(
  db: Database,
  userId: string
): Promise<ApiKey[]> {
  return db
    .select()
    .from(apiKeys)
    .where(and(eq(apiKeys.userId, userId), isNull(apiKeys.revokedAt)));
}

/**
 * List all API keys (admin).
 */
export async function listAllApiKeys(db: Database): Promise<ApiKey[]> {
  return db.select().from(apiKeys).where(isNull(apiKeys.revokedAt));
}

/**
 * Revoke an API key.
 */
export async function revokeApiKey(
  db: Database,
  prefix: string,
  userId?: string
): Promise<boolean> {
  const conditions = [eq(apiKeys.keyPrefix, prefix), isNull(apiKeys.revokedAt)];
  if (userId) {
    conditions.push(eq(apiKeys.userId, userId));
  }

  const result = await db
    .update(apiKeys)
    .set({ revokedAt: new Date() })
    .where(and(...conditions))
    .returning();

  return result.length > 0;
}

/**
 * Update last used timestamp (fire and forget).
 */
export async function updateLastUsed(db: Database, prefix: string): Promise<void> {
  await db
    .update(apiKeys)
    .set({ lastUsedAt: new Date() })
    .where(eq(apiKeys.keyPrefix, prefix));
}
