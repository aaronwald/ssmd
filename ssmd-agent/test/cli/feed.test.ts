import { assertEquals, assertExists } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { listFeeds, showFeed, createFeed } from "../../src/cli/commands/feed.ts";

const FEEDS_DIR = "/workspaces/ssmd/exchanges/feeds";

Deno.test("listFeeds returns feeds from directory", async () => {
  const feeds = await listFeeds(FEEDS_DIR);
  assertEquals(feeds.length >= 1, true);
  const names = feeds.map((f) => f.name);
  assertEquals(names.includes("kalshi"), true);
});

Deno.test("showFeed returns specific feed", async () => {
  const feed = await showFeed(FEEDS_DIR, "kalshi");
  assertExists(feed);
  assertEquals(feed.name, "kalshi");
  assertEquals(feed.type, "websocket");
});

Deno.test("showFeed returns null for missing feed", async () => {
  const feed = await showFeed(FEEDS_DIR, "nonexistent");
  assertEquals(feed, null);
});

Deno.test("createFeed creates new feed file", async () => {
  const tmpDir = await Deno.makeTempDir();

  await createFeed(tmpDir, "test-feed", {
    type: "websocket",
    displayName: "Test Feed",
    endpoint: "wss://test.example.com/ws",
  });

  const feed = await showFeed(tmpDir, "test-feed");
  assertExists(feed);
  assertEquals(feed.name, "test-feed");
  assertEquals(feed.type, "websocket");
  assertEquals(feed.display_name, "Test Feed");

  // Cleanup
  await Deno.remove(tmpDir, { recursive: true });
});
