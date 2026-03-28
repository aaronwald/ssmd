import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { stripContextFromConfig } from "./worker.ts";

Deno.test("stripContextFromConfig: removes _context key", () => {
  const config = {
    function: "archive-freshness",
    params: { threshold: 60 },
    _context: { triggerInfo: { secret: "sk_live_abc" }, stages: {}, input: "", date: "2026-03-28" },
  };
  const result = stripContextFromConfig(config);
  assertEquals(result._context, undefined);
  assertEquals(result.function, "archive-freshness");
  assertEquals(result.params, { threshold: 60 });
});

Deno.test("stripContextFromConfig: no-op when _context absent", () => {
  const config = { url: "http://example.com", method: "GET" };
  const result = stripContextFromConfig(config);
  assertEquals(result, { url: "http://example.com", method: "GET" });
});

Deno.test("stripContextFromConfig: does not mutate original", () => {
  const config = { function: "test", _context: { triggerInfo: {} } };
  const result = stripContextFromConfig(config);
  assertEquals(config._context !== undefined, true);
  assertEquals(result._context, undefined);
});
