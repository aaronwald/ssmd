import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { createRouter, API_VERSION, type RouteContext } from "../../src/server/routes.ts";

// Mock db object for tests that don't use database
const mockDb = {} as RouteContext["db"];

function createTestRouter() {
  const ctx: RouteContext = { dataDir: "/tmp/test-data", db: mockDb };
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
