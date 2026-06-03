import { assertEquals, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { KalshiClient, parseSeriesVolume } from "../../../src/lib/api/kalshi.ts";

Deno.test("parseSeriesVolume prefers volume_fp (stringified float)", () => {
  assertEquals(parseSeriesVolume({ volume_fp: "7083218.00" }), 7083218);
  assertEquals(parseSeriesVolume({ volume_fp: "250000.5" }), 250001);
});

Deno.test("parseSeriesVolume falls back to legacy volume", () => {
  assertEquals(parseSeriesVolume({ volume: 12345 }), 12345);
  // volume_fp wins over legacy volume when both present
  assertEquals(parseSeriesVolume({ volume: 1, volume_fp: "999" }), 999);
});

Deno.test("parseSeriesVolume defaults to 0 on missing/invalid input", () => {
  assertEquals(parseSeriesVolume({}), 0);
  assertEquals(parseSeriesVolume({ volume_fp: "" }), 0);
  assertEquals(parseSeriesVolume({ volume_fp: "not-a-number" }), 0);
});

Deno.test("KalshiClient constructs with production URL", () => {
  const client = new KalshiClient({ apiKey: "test-key" });

  // Can't directly access baseUrl, but we can verify it doesn't throw
  assertEquals(typeof client.fetchAllEvents, "function");
  assertEquals(typeof client.fetchAllMarkets, "function");
});

Deno.test("KalshiClient constructs with demo URL", () => {
  const client = new KalshiClient({ apiKey: "test-key", demo: true });

  assertEquals(typeof client.fetchAllEvents, "function");
});

Deno.test("KalshiClient accepts custom rate limit options", () => {
  const client = new KalshiClient({
    apiKey: "test-key",
    minDelayMs: 500,
    maxRetries: 5,
  });

  assertEquals(typeof client.getEvent, "function");
});

Deno.test("createKalshiClient throws without API key", async () => {
  // Save and clear env
  const original = Deno.env.get("KALSHI_API_KEY");
  Deno.env.delete("KALSHI_API_KEY");

  try {
    // Dynamic import to bypass module caching
    const { createKalshiClient } = await import("../../../src/lib/api/kalshi.ts");
    assertThrows(
      () => createKalshiClient(),
      Error,
      "KALSHI_API_KEY"
    );
  } finally {
    // Restore env
    if (original) {
      Deno.env.set("KALSHI_API_KEY", original);
    }
  }
});

// Note: Integration tests for actual API calls would require mocking
// or a real API key. These are unit tests for the client construction.
