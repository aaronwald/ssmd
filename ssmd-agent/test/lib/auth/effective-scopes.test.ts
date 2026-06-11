import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { selectEffectiveAuth, selectEffectiveUser } from "../../../src/lib/auth/effective-scopes.ts";
import type { ApiKey } from "../../../src/lib/db/schema.ts";

/**
 * Build a minimal ApiKey-shaped fixture. Only the fields used by
 * selectEffectiveAuth / selectEffectiveUser are populated; the rest are cast away.
 */
function makeKey(
  keyPrefix: string,
  scopes: string[],
  createdAt: Date = new Date("2026-01-01T00:00:00Z"),
  allowedFeeds: string[] = ["kalshi"],
  dateRangeStart = "2024-01-01",
  dateRangeEnd = "2026-12-31",
): ApiKey {
  // userId derived from keyPrefix so each fixture key has a distinct, predictable id.
  const userId = `user-${keyPrefix}`;
  return { userId, keyPrefix, scopes, createdAt, allowedFeeds, dateRangeStart, dateRangeEnd } as unknown as ApiKey;
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

// ---- selectEffectiveUser tests ----

Deno.test("selectEffectiveUser returns null for empty array", () => {
  assertEquals(selectEffectiveUser([]), null);
});

Deno.test("selectEffectiveUser single key returns all fields", () => {
  const result = selectEffectiveUser([
    makeKey("sk_live_aaaaaaaa", ["datasets:read"], new Date("2026-01-01"), ["kalshi", "kraken-futures"], "2024-01-01", "2026-12-31"),
  ]);
  assertEquals(result, {
    userId: "user-sk_live_aaaaaaaa",
    keyPrefix: "sk_live_aaaaaaaa",
    scopes: ["datasets:read"],
    allowedFeeds: ["kalshi", "kraken-futures"],
    dateRangeStart: "2024-01-01",
    dateRangeEnd: "2026-12-31",
  });
});

Deno.test("selectEffectiveUser userId comes from the chosen (keyPrefix) key", () => {
  // Wildcard key wins keyPrefix selection; userId must match THAT key, not the others.
  const result = selectEffectiveUser([
    makeKey("sk_live_lowpriv0", ["datasets:read"], new Date("2025-01-01")),
    makeKey("sk_live_wildcard", ["*"], new Date("2026-01-01")),
    makeKey("sk_live_otherkey", ["harman:write"], new Date("2024-01-01")),
  ]);
  assertEquals(result?.keyPrefix, "sk_live_wildcard");
  assertEquals(result?.userId, "user-sk_live_wildcard");
});

Deno.test("selectEffectiveUser unions allowedFeeds across keys", () => {
  const result = selectEffectiveUser([
    makeKey("sk_live_key1xxxx", ["datasets:read"], new Date("2026-01-01"), ["kalshi"], "2025-01-01", "2026-06-30"),
    makeKey("sk_live_key2xxxx", ["datasets:read"], new Date("2026-02-01"), ["kraken-futures"], "2024-01-01", "2027-12-31"),
  ]);
  assertEquals(result?.allowedFeeds, ["kalshi", "kraken-futures"]);
});

Deno.test("selectEffectiveUser uses widest date span (min start, max end)", () => {
  const result = selectEffectiveUser([
    makeKey("sk_live_key1xxxx", ["datasets:read"], new Date("2026-01-01"), ["kalshi"], "2025-06-01", "2026-06-30"),
    makeKey("sk_live_key2xxxx", ["datasets:read"], new Date("2026-02-01"), ["kalshi"], "2024-01-01", "2027-12-31"),
    makeKey("sk_live_key3xxxx", ["datasets:read"], new Date("2026-03-01"), ["kalshi"], "2025-01-01", "2026-12-31"),
  ]);
  assertEquals(result?.dateRangeStart, "2024-01-01");
  assertEquals(result?.dateRangeEnd, "2027-12-31");
});

Deno.test("selectEffectiveUser de-duplicates allowedFeeds", () => {
  const result = selectEffectiveUser([
    makeKey("sk_live_key1xxxx", ["datasets:read"], new Date("2026-01-01"), ["kalshi", "kraken-futures"]),
    makeKey("sk_live_key2xxxx", ["datasets:read"], new Date("2026-02-01"), ["kalshi"]),
  ]);
  assertEquals(result?.allowedFeeds, ["kalshi", "kraken-futures"]);
});

Deno.test("selectEffectiveUser keyPrefix selection follows same rank as selectEffectiveAuth", () => {
  const result = selectEffectiveUser([
    makeKey("sk_live_wildcard", ["*"], new Date("2026-01-01"), ["kalshi"]),
    makeKey("sk_live_lowpriv0", ["datasets:read"], new Date("2025-01-01"), ["kraken-futures"]),
  ]);
  assertEquals(result?.keyPrefix, "sk_live_wildcard");
  // scopes union includes both
  assertEquals(result?.scopes.includes("*"), true);
  assertEquals(result?.scopes.includes("datasets:read"), true);
  // feeds union across both keys
  assertEquals(result?.allowedFeeds.includes("kalshi"), true);
  assertEquals(result?.allowedFeeds.includes("kraken-futures"), true);
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
