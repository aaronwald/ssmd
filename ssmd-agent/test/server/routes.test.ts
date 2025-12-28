import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { createRouter, API_VERSION, type RouteContext } from "../../src/server/routes.ts";

const TEST_API_KEY = "test-api-key";

function createTestRouter() {
  const ctx: RouteContext = { apiKey: TEST_API_KEY };
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

Deno.test("GET /datasets requires auth", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/datasets");
  const res = await router(req);

  assertEquals(res.status, 401);
});

Deno.test("GET /datasets with valid auth returns 200", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/datasets", {
    headers: { Authorization: `Bearer ${TEST_API_KEY}` },
  });
  const res = await router(req);

  assertEquals(res.status, 200);
});

Deno.test("GET /unknown returns 404", async () => {
  const router = createTestRouter();
  const req = new Request("http://localhost/unknown");
  const res = await router(req);

  assertEquals(res.status, 404);
});
