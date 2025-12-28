import { assertEquals, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  FeedSchema,
  getLatestVersion,
  getVersionForDate,
  type Feed,
} from "../../../src/lib/types/feed.ts";

Deno.test("FeedSchema validates valid feed", () => {
  const feed: Feed = {
    name: "kalshi",
    type: "websocket",
    status: "active",
    versions: [
      {
        version: "v1",
        effective_from: "2025-01-01",
        endpoint: "wss://api.kalshi.com/trade-api/ws/v2",
        protocol: { transport: "wss", message: "json" },
      },
    ],
  };

  const result = FeedSchema.parse(feed);
  assertEquals(result.name, "kalshi");
  assertEquals(result.type, "websocket");
  assertEquals(result.versions.length, 1);
});

Deno.test("FeedSchema rejects invalid type", () => {
  const feed = {
    name: "test",
    type: "invalid",
    status: "active",
    versions: [
      {
        version: "v1",
        effective_from: "2025-01-01",
        endpoint: "wss://example.com",
        protocol: { transport: "wss", message: "json" },
      },
    ],
  };

  assertThrows(() => FeedSchema.parse(feed));
});

Deno.test("FeedSchema rejects empty name", () => {
  const feed = {
    name: "",
    type: "websocket",
    versions: [
      {
        version: "v1",
        effective_from: "2025-01-01",
        endpoint: "wss://example.com",
        protocol: { transport: "wss", message: "json" },
      },
    ],
  };

  assertThrows(() => FeedSchema.parse(feed));
});

Deno.test("FeedSchema rejects empty versions", () => {
  const feed = {
    name: "test",
    type: "websocket",
    versions: [],
  };

  assertThrows(() => FeedSchema.parse(feed));
});

Deno.test("FeedSchema rejects invalid date format", () => {
  const feed = {
    name: "test",
    type: "websocket",
    versions: [
      {
        version: "v1",
        effective_from: "01-01-2025", // Wrong format
        endpoint: "wss://example.com",
        protocol: { transport: "wss", message: "json" },
      },
    ],
  };

  assertThrows(() => FeedSchema.parse(feed));
});

Deno.test("getLatestVersion returns most recent version", () => {
  const feed: Feed = {
    name: "test",
    type: "websocket",
    status: "active",
    versions: [
      {
        version: "v1",
        effective_from: "2024-01-01",
        endpoint: "wss://v1.example.com",
        protocol: { transport: "wss", message: "json" },
      },
      {
        version: "v2",
        effective_from: "2025-01-01",
        endpoint: "wss://v2.example.com",
        protocol: { transport: "wss", message: "json" },
      },
    ],
  };

  const latest = getLatestVersion(feed);
  assertEquals(latest?.version, "v2");
  assertEquals(latest?.endpoint, "wss://v2.example.com");
});

Deno.test("getVersionForDate returns correct version", () => {
  const feed: Feed = {
    name: "test",
    type: "websocket",
    status: "active",
    versions: [
      {
        version: "v1",
        effective_from: "2024-01-01",
        effective_to: "2024-12-31",
        endpoint: "wss://v1.example.com",
        protocol: { transport: "wss", message: "json" },
      },
      {
        version: "v2",
        effective_from: "2025-01-01",
        endpoint: "wss://v2.example.com",
        protocol: { transport: "wss", message: "json" },
      },
    ],
  };

  // Date in v1 range
  const v1 = getVersionForDate(feed, new Date("2024-06-15"));
  assertEquals(v1?.version, "v1");

  // Date in v2 range
  const v2 = getVersionForDate(feed, new Date("2025-06-15"));
  assertEquals(v2?.version, "v2");
});

Deno.test("FeedSchema defaults status to active", () => {
  const feed = {
    name: "test",
    type: "websocket",
    versions: [
      {
        version: "v1",
        effective_from: "2025-01-01",
        endpoint: "wss://example.com",
        protocol: { transport: "wss", message: "json" },
      },
    ],
  };

  const result = FeedSchema.parse(feed);
  assertEquals(result.status, "active");
});
