import { assert, assertEquals, assertRejects, assertStringIncludes } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { createOneTimeSecret } from "../../../src/lib/ots/mod.ts";

Deno.test("authenticated share returns link and sends Basic auth", async () => {
  const orig = globalThis.fetch;
  // deno-lint-ignore no-explicit-any
  const captured: { auth: string | null } = { auth: null };
  // deno-lint-ignore no-explicit-any
  (globalThis as any).fetch = (_url: unknown, init: RequestInit | undefined) => {
    captured.auth = new Headers(init?.headers).get("Authorization");
    return Promise.resolve(new Response(JSON.stringify({ secret_key: "abc123" }), { status: 200 }));
  };
  try {
    const link = await createOneTimeSecret("sk_live_x", { username: "u", apiToken: "t", ttlSeconds: 604800 });
    assertEquals(link, "https://onetimesecret.com/secret/abc123");
    assert(captured.auth !== null && captured.auth.startsWith("Basic "));
  } finally { globalThis.fetch = orig; }
});

Deno.test("anonymous share returns link and sends no Authorization header", async () => {
  const orig = globalThis.fetch;
  let capturedHeaders: Headers | null = null;
  // deno-lint-ignore no-explicit-any
  (globalThis as any).fetch = (_url: unknown, init: RequestInit | undefined) => {
    capturedHeaders = new Headers(init?.headers);
    return Promise.resolve(new Response(JSON.stringify({ secret_key: "xyz789" }), { status: 200 }));
  };
  try {
    const link = await createOneTimeSecret("my-secret", { ttlSeconds: 3600 });
    assertEquals(link, "https://onetimesecret.com/secret/xyz789");
    assert(capturedHeaders !== null);
    assertEquals((capturedHeaders as Headers).get("Authorization"), null);
  } finally { globalThis.fetch = orig; }
});

Deno.test("non-2xx response rejects with error mentioning onetimesecret", async () => {
  const orig = globalThis.fetch;
  // deno-lint-ignore no-explicit-any
  (globalThis as any).fetch = (_url: unknown, _init: unknown) => {
    return Promise.resolve(new Response("Unauthorized", { status: 401 }));
  };
  try {
    await assertRejects(
      () => createOneTimeSecret("secret", { ttlSeconds: 3600 }),
      Error,
      "onetimesecret",
    );
  } finally { globalThis.fetch = orig; }
});

Deno.test("200 response with missing secret_key rejects with error mentioning secret_key", async () => {
  const orig = globalThis.fetch;
  // deno-lint-ignore no-explicit-any
  (globalThis as any).fetch = (_url: unknown, _init: unknown) => {
    return Promise.resolve(new Response(JSON.stringify({}), { status: 200 }));
  };
  try {
    await assertRejects(
      () => createOneTimeSecret("secret", { ttlSeconds: 3600 }),
      Error,
      "secret_key",
    );
  } finally { globalThis.fetch = orig; }
});
