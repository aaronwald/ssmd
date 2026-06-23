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

// --- GET /v1/internal/ohlcv-rest-bars (external REST OHLCV normalizer) ---

// Build a router whose auth always succeeds with admin:read scope. The route
// hits external REST APIs, so each test stubs globalThis.fetch.
function createRestBarsRouter() {
  const ctx: RouteContext = {
    dataDir: "/tmp/test-data",
    db: mockDb,
    harmanPools: new Map(),
    authOverride: () =>
      Promise.resolve({
        valid: true,
        userId: "u1",
        userEmail: "test@example.com",
        scopes: ["admin:read"],
        keyPrefix: "test_pref",
        allowedFeeds: ["*"],
        billable: false,
      }),
  };
  return createRouter(ctx);
}

// Stub globalThis.fetch with a function, run fn, then restore.
async function withStubbedFetch(
  stub: (input: string | URL | Request) => Promise<Response>,
  fn: () => Promise<void>,
): Promise<void> {
  const original = globalThis.fetch;
  globalThis.fetch = ((input: string | URL | Request) =>
    stub(input)) as typeof fetch;
  try {
    await fn();
  } finally {
    globalThis.fetch = original;
  }
}

Deno.test("GET /v1/internal/ohlcv-rest-bars polygon maps results to normalized bars", async () => {
  const polygonBody = {
    results: [
      { t: 1_700_000_000_000, o: 1, h: 2, l: 0.5, c: 1.5, v: 100 },
      { t: 1_700_000_060_000, o: 1.5, h: 2.5, l: 1, c: 2, v: 200 },
    ],
  };
  await withStubbedFetch(
    (input) => {
      const u = input.toString();
      assertEquals(u.includes("api.polygon.io"), true);
      assertEquals(u.includes("/AAPL/"), true);
      return Promise.resolve(
        new Response(JSON.stringify(polygonBody), { status: 200 }),
      );
    },
    async () => {
      const router = createRestBarsRouter();
      const req = new Request(
        "http://localhost/v1/internal/ohlcv-rest-bars?source=polygon&sym=AAPL&date=2023-11-14",
        { headers: { "X-API-Key": "test_pref.secret" } },
      );
      // Ensure MASSIVE_API_KEY is set so the route does not 500.
      Deno.env.set("MASSIVE_API_KEY", "test-massive-key");
      const res = await router(req);
      assertEquals(res.status, 200);
      const body = await res.json();
      assertEquals(body.sym, "AAPL");
      assertEquals(body.bars.length, 2);
      assertEquals(body.bars[0], {
        o: 1,
        h: 2,
        l: 0.5,
        c: 1.5,
        v: 100,
        start_ts_ms: 1_700_000_000_000,
      });
      assertEquals(body.bars[1].start_ts_ms, 1_700_000_060_000);
    },
  );
});

Deno.test("GET /v1/internal/ohlcv-rest-bars polygon returns empty bars when results missing", async () => {
  await withStubbedFetch(
    () =>
      Promise.resolve(
        new Response(JSON.stringify({ status: "OK" }), { status: 200 }),
      ),
    async () => {
      const router = createRestBarsRouter();
      Deno.env.set("MASSIVE_API_KEY", "test-massive-key");
      const req = new Request(
        "http://localhost/v1/internal/ohlcv-rest-bars?source=polygon&sym=AAPL&date=2023-11-14",
        { headers: { "X-API-Key": "test_pref.secret" } },
      );
      const res = await router(req);
      assertEquals(res.status, 200);
      const body = await res.json();
      assertEquals(body.sym, "AAPL");
      assertEquals(body.bars, []);
    },
  );
});

Deno.test("GET /v1/internal/ohlcv-rest-bars kraken maps candle arrays, coercing strings and seconds", async () => {
  const krakenBody = {
    error: [],
    result: {
      XXBTZUSD: [
        ["1700000000", "1.0", "2.0", "0.5", "1.5", "1.4", "10.0", 5],
        ["1700000060", "1.5", "2.5", "1.0", "2.0", "1.9", "20.0", 8],
      ],
      last: 1700000060,
    },
  };
  await withStubbedFetch(
    (input) => {
      const u = input.toString();
      assertEquals(u.includes("api.kraken.com"), true);
      assertEquals(u.includes("pair=XBTUSD"), true);
      return Promise.resolve(
        new Response(JSON.stringify(krakenBody), { status: 200 }),
      );
    },
    async () => {
      const router = createRestBarsRouter();
      const req = new Request(
        "http://localhost/v1/internal/ohlcv-rest-bars?source=kraken&sym=XBTUSD",
        { headers: { "X-API-Key": "test_pref.secret" } },
      );
      const res = await router(req);
      assertEquals(res.status, 200);
      const body = await res.json();
      assertEquals(body.sym, "XBTUSD");
      assertEquals(body.bars.length, 2);
      assertEquals(body.bars[0], {
        o: 1.0,
        h: 2.0,
        l: 0.5,
        c: 1.5,
        v: 10.0,
        start_ts_ms: 1_700_000_000_000,
      });
      assertEquals(body.bars[1].start_ts_ms, 1_700_000_060_000);
      assertEquals(typeof body.bars[0].o, "number");
    },
  );
});

Deno.test("GET /v1/internal/ohlcv-rest-bars kraken returns 502 on kraken error", async () => {
  await withStubbedFetch(
    () =>
      Promise.resolve(
        new Response(
          JSON.stringify({ error: ["EQuery:Unknown asset pair"], result: {} }),
          { status: 200 },
        ),
      ),
    async () => {
      const router = createRestBarsRouter();
      const req = new Request(
        "http://localhost/v1/internal/ohlcv-rest-bars?source=kraken&sym=NOPE",
        { headers: { "X-API-Key": "test_pref.secret" } },
      );
      const res = await router(req);
      assertEquals(res.status, 502);
      const body = await res.json();
      assertEquals(body.error.includes("EQuery:Unknown asset pair"), true);
    },
  );
});

Deno.test("GET /v1/internal/ohlcv-rest-bars returns 400 for unknown source", async () => {
  const router = createRestBarsRouter();
  const req = new Request(
    "http://localhost/v1/internal/ohlcv-rest-bars?source=bogus&sym=AAPL&date=2023-11-14",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error.includes("source"), true);
});

Deno.test("GET /v1/internal/ohlcv-rest-bars returns 400 when sym is missing", async () => {
  const router = createRestBarsRouter();
  const req = new Request(
    "http://localhost/v1/internal/ohlcv-rest-bars?source=polygon&date=2023-11-14",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error.includes("sym"), true);
});

Deno.test("GET /v1/internal/ohlcv-rest-bars returns 400 when polygon date is missing", async () => {
  const router = createRestBarsRouter();
  const req = new Request(
    "http://localhost/v1/internal/ohlcv-rest-bars?source=polygon&sym=AAPL",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error.includes("date"), true);
});

Deno.test("GET /v1/internal/ohlcv-rest-bars returns 400 when date is malformed", async () => {
  const router = createRestBarsRouter();
  const req = new Request(
    "http://localhost/v1/internal/ohlcv-rest-bars?source=polygon&sym=AAPL&date=11-14-2023",
    { headers: { "X-API-Key": "test_pref.secret" } },
  );
  const res = await router(req);
  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(body.error.includes("date"), true);
});

Deno.test("GET /v1/internal/ohlcv-rest-bars returns 401 without API key", async () => {
  const router = createTestRouter();
  const req = new Request(
    "http://localhost/v1/internal/ohlcv-rest-bars?source=polygon&sym=AAPL&date=2023-11-14",
  );
  const res = await router(req);
  assertEquals(res.status, 401);
});

Deno.test("GET /v1/internal/ohlcv-rest-bars returns only the most recent `limit` bars (default 120)", async () => {
  // 200 minute bars; default limit (120) must return the newest 120, in order.
  const results = Array.from({ length: 200 }, (_, i) => ({
    t: 1_700_000_000_000 + i * 60_000,
    o: i,
    h: i,
    l: i,
    c: i,
    v: i + 1,
  }));
  await withStubbedFetch(
    () => Promise.resolve(new Response(JSON.stringify({ results }), { status: 200 })),
    async () => {
      const router = createRestBarsRouter();
      Deno.env.set("MASSIVE_API_KEY", "test-massive-key");

      const dfltRes = await router(
        new Request(
          "http://localhost/v1/internal/ohlcv-rest-bars?source=polygon&sym=AAPL&date=2023-11-14",
          { headers: { "X-API-Key": "test_pref.secret" } },
        ),
      );
      assertEquals(dfltRes.status, 200);
      const dflt = await dfltRes.json();
      assertEquals(Array.isArray(dflt.bars), true);
      assertEquals(dflt.bars.length, 120);
      assertEquals(
        dflt.bars[dflt.bars.length - 1].start_ts_ms,
        1_700_000_000_000 + 199 * 60_000,
      );
      assertEquals(dflt.bars[0].start_ts_ms, 1_700_000_000_000 + 80 * 60_000);

      const fiveRes = await router(
        new Request(
          "http://localhost/v1/internal/ohlcv-rest-bars?source=polygon&sym=AAPL&date=2023-11-14&limit=5",
          { headers: { "X-API-Key": "test_pref.secret" } },
        ),
      );
      assertEquals(fiveRes.status, 200);
      const five = await fiveRes.json();
      assertEquals(Array.isArray(five.bars), true);
      assertEquals(five.bars.length, 5);
      assertEquals(five.bars[0].start_ts_ms, 1_700_000_000_000 + 195 * 60_000);
    },
  );
});

Deno.test("GET /v1/internal/ohlcv-rest-bars returns 400 for non-integer limit", async () => {
  const router = createRestBarsRouter();
  const res = await router(
    new Request(
      "http://localhost/v1/internal/ohlcv-rest-bars?source=kraken&sym=BTC/USDT&limit=abc",
      { headers: { "X-API-Key": "test_pref.secret" } },
    ),
  );
  assertEquals(res.status, 400);
  const body = await res.json();
  assertEquals(typeof body.error === "string" && body.error.includes("limit"), true);
});
