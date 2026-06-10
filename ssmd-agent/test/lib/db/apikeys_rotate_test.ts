import { assertEquals, assertMatch, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { rotateApiKeySecret } from "../../../src/lib/db/apikeys.ts";
import type { Database } from "../../../src/lib/db/client.ts";

function fakeDb(returningRows: unknown[]) {
  const calls: string[] = [];
  const chain = {
    set(_val: unknown) { return chain; },
    where(_cond: unknown) { return chain; },
    values(_val: unknown) { return Promise.resolve(); },
    returning() { return Promise.resolve(returningRows); },
  };
  return {
    db: {
      update(_table: unknown) { calls.push("update"); return chain; },
      insert(_table: unknown) { calls.push("insert"); return chain; },
    } as unknown as Database,
    calls,
  };
}

Deno.test("rotateApiKeySecret - success: returns new prefix and fullKey", async () => {
  const { db, calls } = fakeDb([{
    keyPrefix: "sk_live_NEWPREFIX",
    userEmail: "x@y.z",
    scopes: ["datasets:read"],
    allowedFeeds: ["hols"],
  }]);

  const result = await rotateApiKeySecret(db, "sk_live_OLDPREFIX", "actor@test.com");

  // fullKey must start with sk_live_
  assertMatch(result.fullKey, /^sk_live_/);
  // prefix must start with sk_live_
  assertMatch(result.prefix, /^sk_live_/);
  // prefix must differ from the input
  assertEquals(result.prefix !== "sk_live_OLDPREFIX", true);
  // update must have been called
  assertEquals(calls.includes("update"), true);
});

Deno.test("rotateApiKeySecret - not found: rejects with error containing 'not found'", async () => {
  const { db } = fakeDb([]);

  await assertRejects(
    () => rotateApiKeySecret(db, "sk_live_MISSING", "actor@test.com"),
    Error,
    "not found",
  );
});
