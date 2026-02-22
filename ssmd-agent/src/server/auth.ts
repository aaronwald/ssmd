/**
 * Server authentication module
 * Validates API keys against Redis cache and PostgreSQL fallback
 */
import {
  parseApiKey,
  verifySecret,
  getCachedKeyInfo,
  cacheKeyInfo,
  checkRateLimit,
  incrementRateLimitHits,
  type CachedKeyInfo,
} from "../lib/auth/mod.ts";
import { getApiKeyByPrefix, updateLastUsed, type Database } from "../lib/db/mod.ts";

export interface AuthResult {
  valid: boolean;
  status?: number;
  error?: string;
  userId?: string;
  userEmail?: string;
  scopes?: string[];
  keyPrefix?: string;
  rateLimitRemaining?: number;
  rateLimitResetAt?: number;
  allowedFeeds?: string[];
  dateRangeStart?: string;
  dateRangeEnd?: string;
  billable?: boolean;
  disabledAt?: string | null;
}

/**
 * Validate API key and check rate limits.
 */
export async function validateApiKey(
  apiKeyHeader: string | null,
  db: Database
): Promise<AuthResult> {
  if (!apiKeyHeader) {
    return { valid: false, status: 401, error: "Missing API key" };
  }

  // Parse the key
  const parsed = parseApiKey(apiKeyHeader);
  if (!parsed) {
    return { valid: false, status: 401, error: "Invalid API key format" };
  }

  const { prefix, secret } = parsed;

  // Check cache first
  let keyInfo: CachedKeyInfo | null = await getCachedKeyInfo(prefix);

  // Cache miss - check database
  if (!keyInfo) {
    const dbKey = await getApiKeyByPrefix(db, prefix);
    if (!dbKey) {
      return { valid: false, status: 401, error: "API key not found" };
    }

    keyInfo = {
      keyHash: dbKey.keyHash,
      userId: dbKey.userId,
      userEmail: dbKey.userEmail,
      scopes: dbKey.scopes,
      rateLimitTier: dbKey.rateLimitTier,
      revoked: dbKey.revokedAt !== null,
      expiresAt: dbKey.expiresAt?.toISOString() ?? null,
      allowedFeeds: dbKey.allowedFeeds,
      dateRangeStart: dbKey.dateRangeStart,
      dateRangeEnd: dbKey.dateRangeEnd,
      billable: dbKey.billable,
      disabledAt: dbKey.disabledAt?.toISOString() ?? null,
    };

    // Cache for next time
    await cacheKeyInfo(prefix, keyInfo);
  }

  // Check if revoked
  if (keyInfo.revoked) {
    return { valid: false, status: 401, error: "API key revoked" };
  }

  // Check if disabled
  if (keyInfo.disabledAt) {
    return { valid: false, status: 403, error: "API key disabled" };
  }

  // Check if expired
  if (keyInfo.expiresAt && new Date(keyInfo.expiresAt) < new Date()) {
    return { valid: false, status: 401, error: "API key expired" };
  }

  // Verify secret
  if (!(await verifySecret(secret, keyInfo.keyHash))) {
    return { valid: false, status: 401, error: "Invalid API key" };
  }

  // Check rate limit
  const rateLimit = await checkRateLimit(prefix, keyInfo.rateLimitTier);
  if (!rateLimit.allowed) {
    await incrementRateLimitHits(prefix);
    return {
      valid: false,
      status: 429,
      error: "Rate limit exceeded",
      rateLimitRemaining: rateLimit.remaining,
      rateLimitResetAt: rateLimit.resetAt,
    };
  }

  // Update last used (fire and forget)
  updateLastUsed(db, prefix).catch(() => {});

  return {
    valid: true,
    userId: keyInfo.userId,
    userEmail: keyInfo.userEmail,
    scopes: keyInfo.scopes,
    keyPrefix: prefix,
    rateLimitRemaining: rateLimit.remaining,
    rateLimitResetAt: rateLimit.resetAt,
    allowedFeeds: keyInfo.allowedFeeds,
    dateRangeStart: keyInfo.dateRangeStart,
    dateRangeEnd: keyInfo.dateRangeEnd,
    billable: keyInfo.billable,
    disabledAt: keyInfo.disabledAt,
  };
}

/**
 * Check if scopes include required scope.
 */
export function hasScope(scopes: string[], required: string): boolean {
  // admin:write implies admin:read
  if (required === "admin:read" && scopes.includes("admin:write")) {
    return true;
  }
  // signals:write implies signals:read
  if (required === "signals:read" && scopes.includes("signals:write")) {
    return true;
  }
  // billing:write implies billing:read
  if (required === "billing:read" && scopes.includes("billing:write")) {
    return true;
  }
  // admin:read/write implies billing:read (admins can view billing)
  if (required === "billing:read" && (scopes.includes("admin:read") || scopes.includes("admin:write"))) {
    return true;
  }
  // admin:write implies billing:write (admins can issue credits)
  if (required === "billing:write" && scopes.includes("admin:write")) {
    return true;
  }
  // Direct match or wildcard
  return scopes.includes(required) || scopes.includes("*");
}
