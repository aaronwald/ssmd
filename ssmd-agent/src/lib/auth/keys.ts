import { encodeHex } from "https://deno.land/std@0.224.0/encoding/hex.ts";

/**
 * Generate a new API key with prefix and hash.
 * Returns the full key (shown once), prefix (for lookup), and hash (for storage).
 */
export async function generateApiKey(environment: "live" | "test" = "live"): Promise<{
  fullKey: string;
  prefix: string;
  hash: string;
}> {
  // Generate random bytes for key ID and secret
  const keyIdBytes = new Uint8Array(6);
  const secretBytes = new Uint8Array(24);
  crypto.getRandomValues(keyIdBytes);
  crypto.getRandomValues(secretBytes);

  // Encode as URL-safe base64
  const keyId = btoa(String.fromCharCode(...keyIdBytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
  const secret = btoa(String.fromCharCode(...secretBytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");

  const prefix = `sk_${environment}_${keyId}`;
  const fullKey = `${prefix}_${secret}`;
  const hash = await hashSecret(secret);

  return { fullKey, prefix, hash };
}

/**
 * Parse an API key into prefix and secret parts.
 * Returns null if the key format is invalid.
 */
export function parseApiKey(fullKey: string): { prefix: string; secret: string } | null {
  // Match: sk_{env}_{keyId}_{secret}
  const match = fullKey.match(/^(sk_(?:live|test)_[a-zA-Z0-9_-]+)_([a-zA-Z0-9_-]+)$/);
  if (!match) {
    return null;
  }
  return { prefix: match[1], secret: match[2] };
}

/**
 * Hash a secret using SHA-256.
 */
export async function hashSecret(secret: string): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode(secret);
  const hashBuffer = await crypto.subtle.digest("SHA-256", data);
  return encodeHex(new Uint8Array(hashBuffer));
}

/**
 * Verify a secret against a stored hash using constant-time comparison.
 */
export async function verifySecret(secret: string, storedHash: string): Promise<boolean> {
  const providedHash = await hashSecret(secret);

  // Constant-time comparison
  if (providedHash.length !== storedHash.length) {
    return false;
  }

  let result = 0;
  for (let i = 0; i < providedHash.length; i++) {
    result |= providedHash.charCodeAt(i) ^ storedHash.charCodeAt(i);
  }
  return result === 0;
}
