/**
 * Effective auth resolution for an email across all of its active API keys.
 *
 * An email may have multiple active API keys with different scopes. Resolving
 * to a single arbitrary key (LIMIT 1 with no ORDER BY) intermittently dropped
 * admin scope. Instead, resolve to the UNION of scopes across all active keys
 * with a deterministic key_prefix.
 */
import { and, eq, isNull } from "drizzle-orm";
import { type ApiKey, apiKeys } from "../db/schema.ts";
import type { Database } from "../db/client.ts";

export interface EffectiveAuth {
  keyPrefix: string;
  scopes: string[];
}

/**
 * Full resolved identity for a proxied user — includes everything needed to
 * replace the service key's auth context in the request.
 *
 * allowedFeeds: UNION of allowedFeeds across all active keys.
 * dateRangeStart/End: widest span (min start, max end) across all active keys.
 */
export interface EffectiveUser {
  keyPrefix: string;
  scopes: string[];
  allowedFeeds: string[];
  dateRangeStart: string;
  dateRangeEnd: string;
}

/**
 * Given all active (non-revoked, non-disabled) API keys for one email,
 * compute the effective auth: the UNION of scopes across keys, plus a
 * deterministic key_prefix. Returns null if there are no keys.
 *
 * Determinism for key_prefix: prefer a key whose scopes include "*",
 * then a key whose scopes include "harman:admin", then the key with the
 * most scopes; tie-break by earliest createdAt, then keyPrefix ascending.
 */
export function selectEffectiveAuth(keys: ApiKey[]): EffectiveAuth | null {
  if (keys.length === 0) {
    return null;
  }

  // Union of scopes across all keys, de-duplicated and sorted for stable output.
  const scopes = [...new Set(keys.flatMap((k) => k.scopes))].sort();

  // Rank for key_prefix selection: lower rank wins.
  const rank = (key: ApiKey): number => {
    if (key.scopes.includes("*")) return 0;
    if (key.scopes.includes("harman:admin")) return 1;
    return 2;
  };

  // Copy before sorting to avoid mutating the input array.
  const chosen = [...keys].sort((a, b) => {
    const rankDiff = rank(a) - rank(b);
    if (rankDiff !== 0) return rankDiff;

    // Within the same rank, prefer the key with the most scopes.
    const scopeCountDiff = b.scopes.length - a.scopes.length;
    if (scopeCountDiff !== 0) return scopeCountDiff;

    // Tie-break by earliest createdAt.
    const createdDiff = a.createdAt.getTime() - b.createdAt.getTime();
    if (createdDiff !== 0) return createdDiff;

    // Final tie-break by keyPrefix ascending.
    return a.keyPrefix < b.keyPrefix ? -1 : a.keyPrefix > b.keyPrefix ? 1 : 0;
  })[0];

  return { keyPrefix: chosen.keyPrefix, scopes };
}

/** Resolve an email to its effective auth across ALL active keys. */
export async function getEffectiveAuthByEmail(
  db: Database,
  email: string,
): Promise<EffectiveAuth | null> {
  const rows = await db
    .select()
    .from(apiKeys)
    .where(
      and(
        eq(apiKeys.userEmail, email),
        isNull(apiKeys.revokedAt),
        isNull(apiKeys.disabledAt),
      ),
    );
  return selectEffectiveAuth(rows);
}

/**
 * Compute the full effective user identity from a set of active API keys.
 *
 * - scopes: UNION across all keys (same as selectEffectiveAuth)
 * - keyPrefix: deterministic selection (same rank as selectEffectiveAuth)
 * - allowedFeeds: UNION of all allowedFeeds, de-duplicated and sorted
 * - dateRangeStart: earliest start date across all keys (widest span)
 * - dateRangeEnd: latest end date across all keys (widest span)
 *
 * Returns null when there are no keys.
 */
export function selectEffectiveUser(keys: ApiKey[]): EffectiveUser | null {
  const base = selectEffectiveAuth(keys);
  if (!base) return null;

  const allFeeds = [...new Set(keys.flatMap((k) => k.allowedFeeds))].sort();

  // Widest date span: min start, max end. Dates are YYYY-MM-DD strings so
  // lexicographic comparison is correct.
  const starts = keys.map((k) => k.dateRangeStart).sort();
  const ends = keys.map((k) => k.dateRangeEnd).sort();
  const dateRangeStart = starts[0];
  const dateRangeEnd = ends[ends.length - 1];

  return {
    keyPrefix: base.keyPrefix,
    scopes: base.scopes,
    allowedFeeds: allFeeds,
    dateRangeStart,
    dateRangeEnd,
  };
}

/**
 * Resolve an email to its full effective user identity across ALL active keys.
 * Returns null when the email has no active keys.
 */
export async function resolveEffectiveUser(
  db: Database,
  email: string,
): Promise<EffectiveUser | null> {
  const rows = await db
    .select()
    .from(apiKeys)
    .where(
      and(
        eq(apiKeys.userEmail, email),
        isNull(apiKeys.revokedAt),
        isNull(apiKeys.disabledAt),
      ),
    );
  return selectEffectiveUser(rows);
}
