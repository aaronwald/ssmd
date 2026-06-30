import { assert, assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { feedAllowed, resolveAllowedFeeds } from "../../src/server/routes.ts";

// Regression for the wildcard feed-auth bug: a key whose allowedFeeds is ["*"]
// must be authorized for every feed, matching how "*" works for scopes. Several
// /v1/data routes previously did a literal allowedFeeds.includes(feed), which
// 403s a "*" key on every feed (the MCP key, allowed_feeds = {*}, hit this).

Deno.test("feedAllowed: '*' key authorizes any feed", () => {
  for (const feed of ["binance", "kalshi", "kraken-spot", "kraken-futures", "anything"]) {
    assert(feedAllowed(["*"], feed), `'*' should authorize ${feed}`);
  }
});

Deno.test("feedAllowed: explicit list authorizes only listed feeds", () => {
  const allowed = ["kalshi", "kraken-spot"];
  assert(feedAllowed(allowed, "kalshi"));
  assert(feedAllowed(allowed, "kraken-spot"));
  assertEquals(feedAllowed(allowed, "binance"), false);
});

Deno.test("feedAllowed: empty allowlist authorizes nothing", () => {
  assertEquals(feedAllowed([], "binance"), false);
  assertEquals(feedAllowed([], "kalshi"), false);
});

Deno.test("resolveAllowedFeeds: '*' key expands to all candidates", () => {
  const candidates = ["kalshi", "kraken-spot", "binance"];
  assertEquals(
    resolveAllowedFeeds(["*"], candidates, (f) => f).sort(),
    [...candidates].sort(),
  );
});

Deno.test("resolveAllowedFeeds: explicit list intersects with candidates", () => {
  const candidates = ["kalshi", "kraken-spot", "binance"];
  assertEquals(
    resolveAllowedFeeds(["kalshi", "binance"], candidates, (f) => f).sort(),
    ["binance", "kalshi"],
  );
});

Deno.test("resolveAllowedFeeds: works on objects via feedOf selector", () => {
  const candidates = [{ feed: "kalshi" }, { feed: "binance" }];
  assertEquals(resolveAllowedFeeds(["*"], candidates, (c) => c.feed).length, 2);
  assertEquals(
    resolveAllowedFeeds(["binance"], candidates, (c) => c.feed),
    [{ feed: "binance" }],
  );
});

Deno.test("resolveAllowedFeeds: empty allowlist yields no candidates", () => {
  assertEquals(resolveAllowedFeeds([], ["kalshi", "binance"], (f) => f), []);
});
