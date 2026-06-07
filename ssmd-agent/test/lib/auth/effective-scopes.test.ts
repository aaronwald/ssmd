import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { selectEffectiveAuth } from "../../../src/lib/auth/effective-scopes.ts";
import type { ApiKey } from "../../../src/lib/db/schema.ts";

/**
 * Build a minimal ApiKey-shaped fixture. Only the fields used by
 * selectEffectiveAuth are populated; the rest are cast away.
 */
function makeKey(
  keyPrefix: string,
  scopes: string[],
  createdAt: Date = new Date("2026-01-01T00:00:00Z"),
): ApiKey {
  return { keyPrefix, scopes, createdAt } as unknown as ApiKey;
}

Deno.test("selectEffectiveAuth returns null for empty array", () => {
  assertEquals(selectEffectiveAuth([]), null);
});

Deno.test("selectEffectiveAuth single key returns its prefix and sorted scopes", () => {
  const result = selectEffectiveAuth([
    makeKey("sk_live_aaaaaaaa", ["datasets:read", "admin:read"]),
  ]);

  assertEquals(result, {
    keyPrefix: "sk_live_aaaaaaaa",
    scopes: ["admin:read", "datasets:read"],
  });
});

Deno.test("selectEffectiveAuth unions scopes and prefers wildcard key prefix", () => {
  const result = selectEffectiveAuth([
    makeKey("sk_live_lowpriv0", ["datasets:read"]),
    makeKey("sk_live_wildcard", ["*"]),
    makeKey("sk_live_harmanwr", ["harman:write", "datasets:read"]),
  ]);

  assertEquals(result, {
    keyPrefix: "sk_live_wildcard",
    scopes: ["*", "datasets:read", "harman:write"],
  });
});

Deno.test("selectEffectiveAuth prefers harman:admin key when no wildcard", () => {
  const result = selectEffectiveAuth([
    makeKey("sk_live_lowpriv0", ["datasets:read"]),
    makeKey("sk_live_adminkey", ["harman:admin", "admin:read"]),
    makeKey("sk_live_otherkey", ["harman:write"]),
  ]);

  assertEquals(result, {
    keyPrefix: "sk_live_adminkey",
    scopes: ["admin:read", "datasets:read", "harman:admin", "harman:write"],
  });
});

Deno.test("selectEffectiveAuth de-duplicates scopes across keys", () => {
  const result = selectEffectiveAuth([
    makeKey("sk_live_keya0000", ["datasets:read", "admin:read"]),
    makeKey("sk_live_keyb0000", ["admin:read", "datasets:read"]),
  ]);

  assertEquals(result?.scopes, ["admin:read", "datasets:read"]);
});

Deno.test("selectEffectiveAuth is deterministic regardless of input order", () => {
  const keys = [
    makeKey(
      "sk_live_lowpriv0",
      ["datasets:read"],
      new Date("2026-02-01T00:00:00Z"),
    ),
    makeKey(
      "sk_live_keyb0000",
      ["harman:write", "admin:read"],
      new Date("2026-01-15T00:00:00Z"),
    ),
    makeKey(
      "sk_live_keya0000",
      ["billing:read", "admin:read"],
      new Date("2026-01-10T00:00:00Z"),
    ),
    makeKey(
      "sk_live_keyc0000",
      ["harman:read"],
      new Date("2026-03-01T00:00:00Z"),
    ),
  ];

  const baseline = selectEffectiveAuth(keys);

  // Reverse order should produce identical output.
  const shuffled = [...keys].reverse();
  const reordered = selectEffectiveAuth(shuffled);

  assertEquals(reordered, baseline);

  // Input arrays must not be mutated.
  assertEquals(keys[0].keyPrefix, "sk_live_lowpriv0");
  assertEquals(shuffled[0].keyPrefix, "sk_live_keyc0000");
});
