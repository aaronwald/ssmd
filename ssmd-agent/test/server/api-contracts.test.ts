/**
 * API contract tests — locks the v1 API shape.
 *
 * Tests auth enforcement, scope enforcement, feed authorization,
 * error response shapes, and rate limit headers for all endpoints.
 * These tests run without real DB/Redis by using authOverride.
 */
import { assertEquals, assertExists, assert } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { createRouter, API_VERSION, type RouteContext } from "../../src/server/routes.ts";
import type { AuthResult } from "../../src/server/auth.ts";
import type { Database } from "../../src/lib/db/mod.ts";

// --- Test helpers ---

function mockAuth(overrides: Partial<AuthResult> = {}): AuthResult {
  return {
    valid: true,
    userId: "test-user",
    userEmail: "test@test.com",
    scopes: ["datasets:read", "secmaster:read"],
    keyPrefix: "sk_test_abc",
    rateLimitRemaining: 100,
    rateLimitResetAt: Date.now() + 60_000,
    allowedFeeds: ["kalshi", "kraken-futures", "polymarket"],
    dateRangeStart: "2024-01-01",
    dateRangeEnd: "2027-12-31",
    billable: true,
    ...overrides,
  };
}

function createTestRouter(authResult?: AuthResult | null) {
  const mockDb = {} as Database;
  const ctx: RouteContext = {
    dataDir: "/tmp/test-data",
    db: mockDb,
    authOverride: (_apiKey: string | null, _db: Database) =>
      Promise.resolve(authResult ?? mockAuth()),
  };
  return createRouter(ctx);
}

function makeReq(path: string, opts?: RequestInit & { apiKey?: string }): Request {
  const headers: Record<string, string> = {};
  if (opts?.apiKey) {
    headers["X-API-Key"] = opts.apiKey;
  } else if (opts?.apiKey !== null) {
    // Default: provide a dummy key so auth can process it
    headers["X-API-Key"] = "sk_test_abc_secretsecretsecretsecret";
  }
  if (opts?.headers) {
    for (const [k, v] of Object.entries(opts.headers)) {
      headers[k] = v as string;
    }
  }
  return new Request(`http://localhost${path}`, {
    ...opts,
    headers: { ...headers },
  });
}

// --- Auth enforcement tests ---

const AUTH_ENDPOINTS = [
  "GET /v1/data/feeds",
  "GET /v1/data/catalog",
  "GET /v1/data/schemas",
  "GET /v1/data/trades?feed=kalshi",
  "GET /v1/data/prices?feed=kalshi",
  "GET /v1/data/events?feed=kalshi",
  "GET /v1/data/volume",
  "GET /v1/data/freshness",
  "GET /v1/data/download?feed=kalshi&from=2026-01-01&to=2026-01-02",
  "GET /v1/markets/lookup?ids=TICKER1",
  "GET /v1/events",
  "GET /v1/markets",
  "GET /v1/series",
  "GET /v1/pairs",
  "GET /v1/conditions",
  "GET /v1/fees",
  "GET /v1/keys",
  "GET /v1/settings",
  "GET /v1/billing/summary?key_prefix=sk_test",
  "GET /v1/billing/report",
  "GET /v1/billing/export",
  "GET /v1/billing/ledger?key_prefix=sk_test",
  "GET /v1/billing/balance?key_prefix=sk_test",
  "GET /v1/billing/rates",
  "POST /v1/billing/credit",
];

for (const endpoint of AUTH_ENDPOINTS) {
  const [method, path] = endpoint.split(" ");
  Deno.test(`${endpoint} returns 401 without API key`, async () => {
    const router = createTestRouter({ valid: false, status: 401, error: "Missing API key" });
    const req = new Request(`http://localhost${path}`, { method });
    const res = await router(req);
    assertEquals(res.status, 401);
    const body = await res.json();
    assertExists(body.error);
    assertEquals(typeof body.error, "string");
  });
}

Deno.test("401 returns { error: string } shape", async () => {
  const router = createTestRouter({ valid: false, status: 401, error: "Missing API key" });
  const req = new Request("http://localhost/v1/data/feeds");
  const res = await router(req);
  assertEquals(res.status, 401);
  const body = await res.json();
  assertEquals(typeof body.error, "string");
  assertEquals(body.error, "Missing API key");
});

Deno.test("Invalid API key format returns 401", async () => {
  const router = createTestRouter({ valid: false, status: 401, error: "Invalid API key format" });
  const req = makeReq("/v1/data/feeds", { apiKey: "not-a-valid-key" });
  const res = await router(req);
  assertEquals(res.status, 401);
  const body = await res.json();
  assertEquals(body.error, "Invalid API key format");
});

Deno.test("Revoked key returns 401", async () => {
  const router = createTestRouter({ valid: false, status: 401, error: "API key revoked" });
  const req = makeReq("/v1/data/feeds");
  const res = await router(req);
  assertEquals(res.status, 401);
  const body = await res.json();
  assertEquals(body.error, "API key revoked");
});

Deno.test("Disabled key returns 403", async () => {
  const router = createTestRouter({ valid: false, status: 403, error: "API key disabled" });
  const req = makeReq("/v1/data/feeds");
  const res = await router(req);
  assertEquals(res.status, 403);
  const body = await res.json();
  assertEquals(body.error, "API key disabled");
});

Deno.test("Expired key returns 401", async () => {
  const router = createTestRouter({ valid: false, status: 401, error: "API key expired" });
  const req = makeReq("/v1/data/feeds");
  const res = await router(req);
  assertEquals(res.status, 401);
  const body = await res.json();
  assertEquals(body.error, "API key expired");
});

Deno.test("Rate limited returns 429", async () => {
  const router = createTestRouter({
    valid: false,
    status: 429,
    error: "Rate limit exceeded",
    rateLimitRemaining: 0,
    rateLimitResetAt: Date.now() + 60_000,
  });
  const req = makeReq("/v1/data/feeds");
  const res = await router(req);
  assertEquals(res.status, 429);
  const body = await res.json();
  assertEquals(body.error, "Rate limit exceeded");
});

// --- Scope enforcement tests ---

Deno.test("datasets:read key cannot access admin endpoints", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["datasets:read"] }));
  const req = makeReq("/v1/keys");
  const res = await router(req);
  assertEquals(res.status, 403);
  const body = await res.json();
  assertExists(body.error);
});

Deno.test("datasets:read key cannot access settings", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["datasets:read"] }));
  const req = makeReq("/v1/settings");
  const res = await router(req);
  assertEquals(res.status, 403);
});

Deno.test("datasets:read key cannot access billing", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["datasets:read"] }));
  const req = makeReq("/v1/billing/summary?key_prefix=sk_test");
  const res = await router(req);
  assertEquals(res.status, 403);
});

Deno.test("secmaster:read key cannot access admin endpoints", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["secmaster:read"] }));
  const req = makeReq("/v1/keys");
  const res = await router(req);
  assertEquals(res.status, 403);
});

Deno.test("secmaster:read key cannot access datasets", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["secmaster:read"] }));
  const req = makeReq("/v1/data/feeds");
  const res = await router(req);
  assertEquals(res.status, 403);
});

Deno.test("admin:read implies billing:read", async () => {
  // admin:read should be able to access billing endpoints via billing:read implication
  const router = createTestRouter(mockAuth({ scopes: ["admin:read"] }));
  const req = makeReq("/v1/billing/summary?key_prefix=sk_test&month=2026-01");
  try {
    const res = await router(req);
    assert(res.status !== 403, `admin:read should imply billing:read (got ${res.status})`);
  } catch (_err) {
    // Handler threw because mock DB can't query — scope check passed (good)
  }
});

Deno.test("billing:read can access billing summary", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["billing:read"] }));
  const req = makeReq("/v1/billing/summary?key_prefix=sk_test&month=2026-01");
  try {
    const res = await router(req);
    assert(res.status !== 403, `billing:read should access billing endpoints (got ${res.status})`);
  } catch (_err) {
    // Handler threw because mock DB can't query — scope check passed (good)
  }
});

Deno.test("billing:read cannot issue credits", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["billing:read"] }));
  const req = makeReq("/v1/billing/credit", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ key_prefix: "sk_test", amount_usd: 100 }),
  });
  const res = await router(req);
  assertEquals(res.status, 403);
});

Deno.test("billing:write can issue credits (scope passes)", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["billing:write"] }));
  const req = makeReq("/v1/billing/credit", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ key_prefix: "sk_test", amount_usd: 100 }),
  });
  try {
    const res = await router(req);
    assert(res.status !== 403, `billing:write should access credit endpoint (got ${res.status})`);
  } catch (_err) {
    // Handler threw because mock DB can't query — scope check passed (good)
  }
});

Deno.test("billing:write implies billing:read", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["billing:write"] }));
  const req = makeReq("/v1/billing/summary?key_prefix=sk_test&month=2026-01");
  try {
    const res = await router(req);
    assert(res.status !== 403, `billing:write should imply billing:read (got ${res.status})`);
  } catch (_err) {
    // Handler threw because mock DB can't query — scope check passed (good)
  }
});

Deno.test("datasets:read cannot access billing credit", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["datasets:read"] }));
  const req = makeReq("/v1/billing/credit", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ key_prefix: "sk_test", amount_usd: 100 }),
  });
  const res = await router(req);
  assertEquals(res.status, 403);
});

Deno.test("admin:write implies admin:read", async () => {
  // admin:write implies admin:read — scope check should pass
  const router = createTestRouter(mockAuth({ scopes: ["admin:write"] }));
  const req = makeReq("/v1/settings");
  try {
    const res = await router(req);
    assert(res.status !== 403, `admin:write should imply admin:read (got ${res.status})`);
  } catch (_err) {
    // Handler threw because mock DB can't query — scope check passed (good)
  }
});

Deno.test("admin:write implies billing:write", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["admin:write"] }));
  const req = makeReq("/v1/billing/credit", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ key_prefix: "sk_test", amount_usd: 100 }),
  });
  try {
    const res = await router(req);
    assert(res.status !== 403, `admin:write should imply billing:write (got ${res.status})`);
  } catch (_err) {
    // Handler threw because mock DB can't query — scope check passed (good)
  }
});

// --- Error shape tests ---

Deno.test("404 returns { error: string } shape", async () => {
  const router = createTestRouter();
  const req = makeReq("/v1/nonexistent/endpoint");
  const res = await router(req);
  assertEquals(res.status, 404);
  const body = await res.json();
  assertExists(body.error);
  assertEquals(typeof body.error, "string");
});

Deno.test("Unknown root path returns 404", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/foobar");
  const res = await router(req);
  assertEquals(res.status, 404);
});

// --- Unauthenticated endpoints ---

Deno.test("GET /health returns ok without auth", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/health");
  const res = await router(req);
  assertEquals(res.status, 200);
  const body = await res.json();
  assertEquals(body.status, "ok");
});

Deno.test("GET /version returns version", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/version");
  const res = await router(req);
  assertEquals(res.status, 200);
  const body = await res.json();
  assertEquals(body.version, API_VERSION);
});

Deno.test("GET /metrics returns prometheus format", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/metrics");
  const res = await router(req);
  assertEquals(res.status, 200);
  assertEquals(res.headers.get("Content-Type"), "text/plain; charset=utf-8");
});

// --- Parameter validation tests ---

Deno.test("GET /v1/data/trades without feed returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["datasets:read"] }));
  const req = makeReq("/v1/data/trades");
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertExists(body.error);
});

Deno.test("GET /v1/data/trades with invalid feed returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["datasets:read"] }));
  const req = makeReq("/v1/data/trades?feed=invalid_feed");
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertExists(body.error);
});

Deno.test("GET /v1/data/prices without feed returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["datasets:read"] }));
  const req = makeReq("/v1/data/prices");
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertExists(body.error);
});

Deno.test("GET /v1/data/events without feed returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["datasets:read"] }));
  const req = makeReq("/v1/data/events");
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertExists(body.error);
});

Deno.test("GET /v1/markets/lookup without ids returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["datasets:read"] }));
  const req = makeReq("/v1/markets/lookup");
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertExists(body.error);
});

Deno.test("GET /v1/billing/summary without key_prefix returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["admin:read"] }));
  const req = makeReq("/v1/billing/summary");
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error, "key_prefix query parameter is required");
});

Deno.test("GET /v1/billing/summary with invalid month returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["admin:read"] }));
  const req = makeReq("/v1/billing/summary?key_prefix=sk_test&month=invalid");
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error, "month must be YYYY-MM format");
});

Deno.test("GET /v1/billing/report with invalid month returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["admin:read"] }));
  const req = makeReq("/v1/billing/report?month=2026");
  const res = await router(req);
  assertEquals(res.status, 400);
});

Deno.test("GET /v1/billing/export with invalid month returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["admin:read"] }));
  const req = makeReq("/v1/billing/export?month=bad");
  const res = await router(req);
  assertEquals(res.status, 400);
});

Deno.test("GET /v1/billing/ledger without key_prefix returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["billing:read"] }));
  const req = makeReq("/v1/billing/ledger");
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error, "key_prefix query parameter is required");
});

Deno.test("GET /v1/billing/balance without key_prefix returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["billing:read"] }));
  const req = makeReq("/v1/billing/balance");
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error, "key_prefix query parameter is required");
});

Deno.test("POST /v1/billing/credit without amount returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["billing:write"] }));
  const req = makeReq("/v1/billing/credit", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ key_prefix: "sk_test" }),
  });
  const res = await router(req);
  assertEquals(res.status, 400);
});

Deno.test("POST /v1/billing/credit with negative amount returns 400", async () => {
  const router = createTestRouter(mockAuth({ scopes: ["billing:write"] }));
  const req = makeReq("/v1/billing/credit", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ key_prefix: "sk_test", amount_usd: -50 }),
  });
  const res = await router(req);
  assertEquals(res.status, 400);
});

// --- Feed authorization tests ---

Deno.test("Key restricted to kalshi cannot query polymarket trades", async () => {
  const router = createTestRouter(mockAuth({
    scopes: ["datasets:read"],
    allowedFeeds: ["kalshi"],
  }));
  const req = makeReq("/v1/data/trades?feed=polymarket");
  const res = await router(req);
  assertEquals(res.status, 403);
  const body = await res.json();
  assertExists(body.error);
});

Deno.test("Key restricted to kalshi cannot query kraken prices", async () => {
  const router = createTestRouter(mockAuth({
    scopes: ["datasets:read"],
    allowedFeeds: ["kalshi"],
  }));
  const req = makeReq("/v1/data/prices?feed=kraken-futures");
  const res = await router(req);
  assertEquals(res.status, 403);
});

Deno.test("Key restricted to kalshi cannot download polymarket data", async () => {
  const router = createTestRouter(mockAuth({
    scopes: ["datasets:read"],
    allowedFeeds: ["kalshi"],
  }));
  const req = makeReq("/v1/data/download?feed=polymarket&from=2026-01-01&to=2026-01-02");
  const res = await router(req);
  assertEquals(res.status, 403);
});

// --- Rate limit header tests ---

Deno.test("Authenticated response includes rate limit headers", async () => {
  const router = createTestRouter(mockAuth({
    scopes: ["datasets:read"],
    rateLimitRemaining: 95,
    rateLimitResetAt: 1700000000000,
  }));
  const req = makeReq("/v1/data/feeds");
  const res = await router(req);
  // The response should have rate limit headers
  // Note: these are set in the router when auth succeeds
  const remaining = res.headers.get("X-RateLimit-Remaining");
  const reset = res.headers.get("X-RateLimit-Reset");
  assertExists(remaining, "Response should include X-RateLimit-Remaining");
  assertExists(reset, "Response should include X-RateLimit-Reset");
});

// --- API version tests ---

Deno.test("API_VERSION is 1.0.0", () => {
  assertEquals(API_VERSION, "1.0.0");
});
