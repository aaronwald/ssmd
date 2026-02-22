/**
 * API keys database operations
 */
import { eq, isNull, and } from "drizzle-orm";
import { apiKeys, apiKeyEvents, type ApiKey, type NewApiKey } from "./schema.ts";
import type { Database } from "./client.ts";

/**
 * Get API key by prefix (for validation).
 * Returns active (non-revoked) keys only.
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
  const created = result[0];

  // Log creation event
  await logKeyEvent(db, created.keyPrefix, "created", created.userEmail, null, {
    scopes: created.scopes,
    rateLimitTier: created.rateLimitTier,
    allowedFeeds: created.allowedFeeds,
    billable: created.billable,
  });

  return created;
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
 * @param includeRevoked - if true, include revoked keys
 */
export async function listAllApiKeys(
  db: Database,
  includeRevoked = false
): Promise<ApiKey[]> {
  if (includeRevoked) {
    return db.select().from(apiKeys);
  }
  return db.select().from(apiKeys).where(isNull(apiKeys.revokedAt));
}

/**
 * Revoke an API key (permanent — cannot be undone).
 */
export async function revokeApiKey(
  db: Database,
  prefix: string,
  actor?: string
): Promise<boolean> {
  const conditions = [eq(apiKeys.keyPrefix, prefix), isNull(apiKeys.revokedAt)];

  const result = await db
    .update(apiKeys)
    .set({ revokedAt: new Date() })
    .where(and(...conditions))
    .returning();

  if (result.length > 0) {
    await logKeyEvent(db, prefix, "revoked", actor ?? "system", null, null);
  }

  return result.length > 0;
}

/**
 * Disable an API key (temporary — can be re-enabled).
 */
export async function disableApiKey(
  db: Database,
  prefix: string,
  actor: string
): Promise<boolean> {
  const result = await db
    .update(apiKeys)
    .set({ disabledAt: new Date() })
    .where(and(
      eq(apiKeys.keyPrefix, prefix),
      isNull(apiKeys.revokedAt),
      isNull(apiKeys.disabledAt)
    ))
    .returning();

  if (result.length > 0) {
    await logKeyEvent(db, prefix, "disabled", actor, null, null);
  }

  return result.length > 0;
}

/**
 * Enable a previously disabled API key.
 */
export async function enableApiKey(
  db: Database,
  prefix: string,
  actor: string
): Promise<boolean> {
  const result = await db
    .update(apiKeys)
    .set({ disabledAt: null })
    .where(and(
      eq(apiKeys.keyPrefix, prefix),
      isNull(apiKeys.revokedAt)
    ))
    .returning();

  if (result.length > 0) {
    await logKeyEvent(db, prefix, "enabled", actor, null, null);
  }

  return result.length > 0;
}

/**
 * Update scopes for an API key.
 */
export async function updateApiKeyScopes(
  db: Database,
  prefix: string,
  scopes: string[],
  actor?: string
): Promise<ApiKey | null> {
  // Get current scopes for audit trail
  const current = await getApiKeyByPrefix(db, prefix);
  const oldScopes = current?.scopes ?? [];

  const result = await db
    .update(apiKeys)
    .set({ scopes })
    .where(and(eq(apiKeys.keyPrefix, prefix), isNull(apiKeys.revokedAt)))
    .returning();

  if (result.length > 0) {
    await logKeyEvent(
      db,
      prefix,
      "scopes_changed",
      actor ?? "system",
      { scopes: oldScopes },
      { scopes }
    );
  }

  return result[0] ?? null;
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

/**
 * Log a key lifecycle event to the audit trail.
 */
export async function logKeyEvent(
  db: Database,
  keyPrefix: string,
  eventType: string,
  actor: string,
  oldValue: Record<string, unknown> | null,
  newValue: Record<string, unknown> | null
): Promise<void> {
  try {
    await db.insert(apiKeyEvents).values({
      keyPrefix,
      eventType,
      actor,
      oldValue,
      newValue,
    });
  } catch (err) {
    // Best effort — don't fail the parent operation
    console.error("Failed to log key event:", err);
  }
}
