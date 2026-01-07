import { assertEquals, assertMatch } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { generateApiKey, parseApiKey, hashSecret, verifySecret } from "../../../src/lib/auth/keys.ts";

Deno.test("generateApiKey returns full key, prefix, and hash", async () => {
  const result = await generateApiKey("live");

  assertMatch(result.fullKey, /^sk_live_[a-zA-Z0-9_-]+_[a-zA-Z0-9_-]+$/);
  assertMatch(result.prefix, /^sk_live_[a-zA-Z0-9_-]+$/);
  assertEquals(result.hash.length, 64); // SHA-256 hex
});

Deno.test("generateApiKey test environment", async () => {
  const result = await generateApiKey("test");

  assertMatch(result.fullKey, /^sk_test_/);
  assertMatch(result.prefix, /^sk_test_/);
});

Deno.test("parseApiKey extracts prefix and secret", () => {
  // keyId must be exactly 8 chars (6 bytes base64)
  const result = parseApiKey("sk_live_abc12345_secretpart");

  assertEquals(result?.prefix, "sk_live_abc12345");
  assertEquals(result?.secret, "secretpart");
});

Deno.test("parseApiKey returns null for invalid format", () => {
  const result = parseApiKey("invalid-key");

  assertEquals(result, null);
});

Deno.test("verifySecret matches hashed secret", async () => {
  const secret = "test-secret-value";
  const hash = await hashSecret(secret);

  assertEquals(await verifySecret(secret, hash), true);
  assertEquals(await verifySecret("wrong-secret", hash), false);
});
