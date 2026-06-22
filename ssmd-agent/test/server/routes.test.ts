import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { createRouter, API_VERSION, type RouteContext } from "../../src/server/routes.ts";

// Mock db object for tests that don't use database
const mockDb = {} as RouteContext["db"];

function createTestRouter() {
  const ctx: RouteContext = { dataDir: "/tmp/test-data", db: mockDb, harmanPools: new Map() };
  return createRouter(ctx);
}

Deno.test("GET /health returns ok without auth", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/health");
  const res = await router(req);

  assertEquals(res.status, 200);
  const body = await res.json();
  assertEquals(body.status, "ok");
});

Deno.test("GET /version returns version without auth", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/version");
  const res = await router(req);

  assertEquals(res.status, 200);
  const body = await res.json();
  assertEquals(body.version, API_VERSION);
});

Deno.test("GET /metrics returns prometheus format without auth", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/metrics");
  const res = await router(req);

  assertEquals(res.status, 200);
  assertEquals(res.headers.get("Content-Type"), "text/plain; charset=utf-8");
});

Deno.test("GET /datasets returns 401 without valid API key", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/datasets");
  const res = await router(req);

  assertEquals(res.status, 401);
  const body = await res.json();
  assertEquals(body.error, "Missing API key");
});

Deno.test("GET /datasets returns 401 with invalid API key format", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/datasets", {
    headers: { "X-API-Key": "invalid-key" },
  });
  const res = await router(req);

  assertEquals(res.status, 401);
  const body = await res.json();
  assertEquals(body.error, "Invalid API key format");
});

Deno.test("GET /datasets returns 401 with invalid Bearer token format", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/datasets", {
    headers: { "Authorization": "Bearer invalid-key" },
  });
  const res = await router(req);

  assertEquals(res.status, 401);
  const body = await res.json();
  assertEquals(body.error, "Invalid API key format");
});

Deno.test("GET /v1/events returns 401 without valid API key", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/v1/events");
  const res = await router(req);

  assertEquals(res.status, 401);
});

Deno.test("GET /v1/markets returns 401 without valid API key", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/v1/markets");
  const res = await router(req);

  assertEquals(res.status, 401);
});

Deno.test("GET /v1/markets/lookup returns 401 without valid API key", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/v1/markets/lookup?ids=TICKER1");
  const res = await router(req);

  assertEquals(res.status, 401);
});

Deno.test("GET /v1/keys returns 401 without valid API key", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/v1/keys");
  const res = await router(req);

  assertEquals(res.status, 401);
});

Deno.test("GET /unknown returns 404", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/unknown");
  const res = await router(req);

  assertEquals(res.status, 404);
});

Deno.test("GET /v1/settings returns 401 without API key", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/v1/settings");
  const res = await router(req);
  assertEquals(res.status, 401);
});

Deno.test("PUT /v1/settings/:key returns 401 without API key", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/v1/settings/test_key", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ value: true }),
  });
  const res = await router(req);
  assertEquals(res.status, 401);
});

Deno.test("POST /v1/chat/completions returns 401 without API key", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/v1/chat/completions", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ model: "test", messages: [] }),
  });
  const res = await router(req);
  assertEquals(res.status, 401);
});

// --- GET /v1/data/ohlcv/1m (bar-cache Redis ring) ---

// Build a router whose auth always succeeds with the given allowed feeds, and
// whose Redis returns a canned map of key → value. Mirrors the authOverride
// pattern already used for other route tests.
function createBarCacheRouter(
  allowedFeeds: string[],
  store: Record<string, string>,
) {
  const ctx: RouteContext = {
    dataDir: "/tmp/test-data",
    db: mockDb,
    harmanPools: new Map(),
    authOverride: () =>
      Promise.resolve({
        valid: true,
        userId: "u1",
        userEmail: "test@example.com",
        scopes: ["datasets:read"],
        keyPrefix: "test_pref",
        allowedFeeds,
        billable: false,
      }),
    redisOverride: {
      get: (key: string) => Promise.resolve(key in store ? store[key] : null),
    },
  };
  return createRouter(ctx);
}

function bar(startMs: number, close: number) {
  return {
    sym: "AAPL",
    o: close - 1,
    h: close + 1,
    l: close - 2,
    c: close,
    v: 100,
    start_ts_ms: startMs,
    end_ts_ms: startMs + 60_000,
  };
}

Deno.test("GET /v1/data/ohlcv/1m returns bars and served_at for a seeded key", async () => {
  const bars = [bar(1_000, 10), bar(61_000, 11), bar(121_000, 12)];
  const router = createBarCacheRouter(["massive"], {
    "ohlcv_1m:massive:AAPL": JSON.stringify(bars),
  });

  const req = new Request(
    "http://localhost/v1/data/ohlcv/1m?feed=massive&sym=AAPL",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);

  assertEquals(res.status, 200);
  const body = await res.json();
  assertEquals(body.feed, "massive");
  assertEquals(body.sym, "AAPL");
  assertEquals(body.bars.length, 3);
  assertEquals(body.bars[0].c, 10);
  assertEquals(body.bars[2].c, 12);
  assertEquals(typeof body.served_at, "string");
  // served_at must be a valid ISO timestamp
  assertEquals(Number.isNaN(Date.parse(body.served_at)), false);
});

Deno.test("GET /v1/data/ohlcv/1m clamps to the last `limit` bars", async () => {
  const bars = [bar(1_000, 10), bar(61_000, 11), bar(121_000, 12)];
  const router = createBarCacheRouter(["massive"], {
    "ohlcv_1m:massive:AAPL": JSON.stringify(bars),
  });

  const req = new Request(
    "http://localhost/v1/data/ohlcv/1m?feed=massive&sym=AAPL&limit=2",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);

  assertEquals(res.status, 200);
  const body = await res.json();
  // Last 2 bars, oldest→newest
  assertEquals(body.bars.length, 2);
  assertEquals(body.bars[0].c, 11);
  assertEquals(body.bars[1].c, 12);
});

Deno.test("GET /v1/data/ohlcv/1m caps limit above 60 at 60", async () => {
  const bars = Array.from({ length: 60 }, (_, i) => bar(i * 60_000, i));
  const router = createBarCacheRouter(["kraken-spot"], {
    "ohlcv_1m:kraken-spot:XBTUSD": JSON.stringify(bars),
  });

  const req = new Request(
    "http://localhost/v1/data/ohlcv/1m?feed=kraken-spot&sym=XBTUSD&limit=1000",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);

  assertEquals(res.status, 200);
  const body = await res.json();
  assertEquals(body.bars.length, 60);
});

Deno.test("GET /v1/data/ohlcv/1m returns 404 when key is missing", async () => {
  const router = createBarCacheRouter(["massive"], {});

  const req = new Request(
    "http://localhost/v1/data/ohlcv/1m?feed=massive&sym=NOPE",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);

  assertEquals(res.status, 404);
  const body = await res.json();
  assertEquals(body.error, "no cached bars");
});

Deno.test("GET /v1/data/ohlcv/1m returns 400 for an invalid feed", async () => {
  const router = createBarCacheRouter(["*"], {});

  const req = new Request(
    "http://localhost/v1/data/ohlcv/1m?feed=kalshi&sym=AAPL",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);

  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error.includes("massive"), true);
  assertEquals(body.error.includes("kraken-spot"), true);
});

Deno.test("GET /v1/data/ohlcv/1m returns 400 when sym is missing", async () => {
  const router = createBarCacheRouter(["massive"], {});

  const req = new Request(
    "http://localhost/v1/data/ohlcv/1m?feed=massive",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);

  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error, "sym query parameter is required");
});

Deno.test("GET /v1/data/ohlcv/1m returns 403 when feed is not authorized", async () => {
  const router = createBarCacheRouter(["kraken-spot"], {
    "ohlcv_1m:massive:AAPL": JSON.stringify([bar(1_000, 10)]),
  });

  const req = new Request(
    "http://localhost/v1/data/ohlcv/1m?feed=massive&sym=AAPL",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);

  assertEquals(res.status, 403);
  const body = await res.json();
  assertEquals(body.error, "Key not authorized for feed: massive");
});

Deno.test("GET /v1/data/ohlcv/1m allows wildcard feed access", async () => {
  const router = createBarCacheRouter(["*"], {
    "ohlcv_1m:massive:AAPL": JSON.stringify([bar(1_000, 10)]),
  });

  const req = new Request(
    "http://localhost/v1/data/ohlcv/1m?feed=massive&sym=AAPL",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);

  assertEquals(res.status, 200);
});

Deno.test("GET /v1/data/ohlcv/1m returns 401 without API key", async () => {
  const router = createTestRouter();
  const req = new Request(
    "http://localhost/v1/data/ohlcv/1m?feed=massive&sym=AAPL",
  );
  const res = await router(req);
  assertEquals(res.status, 401);
});
