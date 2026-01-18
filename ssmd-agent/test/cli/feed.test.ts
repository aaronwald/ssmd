import { assertEquals, assertExists } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { listFeeds, showFeed, createFeed } from "../../src/cli/commands/feed.ts";

Deno.test("listFeeds returns feeds from directory", async () => {
  const tmpDir = await Deno.makeTempDir();

  // Create a test feed
  await createFeed(tmpDir, "test-feed", {
    type: "websocket",
    displayName: "Test Feed",
    endpoint: "wss://test.example.com/ws",
  });

  const feeds = await listFeeds(tmpDir);
  assertEquals(feeds.length >= 1, true);
  const names = feeds.map((f) => f.name);
  assertEquals(names.includes("test-feed"), true);

  await Deno.remove(tmpDir, { recursive: true });
});

Deno.test("showFeed returns specific feed", async () => {
  const tmpDir = await Deno.makeTempDir();

  await createFeed(tmpDir, "kalshi", {
    type: "websocket",
    displayName: "Kalshi",
    endpoint: "wss://kalshi.com/ws",
  });

  const feed = await showFeed(tmpDir, "kalshi");
  assertExists(feed);
  assertEquals(feed.name, "kalshi");
  assertEquals(feed.type, "websocket");

  await Deno.remove(tmpDir, { recursive: true });
});

Deno.test("showFeed returns null for missing feed", async () => {
  const tmpDir = await Deno.makeTempDir();
  const feed = await showFeed(tmpDir, "nonexistent");
  assertEquals(feed, null);
  await Deno.remove(tmpDir, { recursive: true });
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

  await Deno.remove(tmpDir, { recursive: true });
});
