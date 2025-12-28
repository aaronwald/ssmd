import { assertEquals, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { BacktestManifestSchema, BacktestResultSchema } from "../../src/lib/types/backtest.ts";

Deno.test("BacktestManifestSchema validates with dates array", () => {
  const manifest = {
    feed: "kalshi",
    dates: ["2025-12-25", "2025-12-26"],
  };

  const result = BacktestManifestSchema.parse(manifest);
  assertEquals(result.dates, ["2025-12-25", "2025-12-26"]);
});

Deno.test("BacktestManifestSchema validates with date_range", () => {
  const manifest = {
    feed: "kalshi",
    date_range: { from: "2025-12-20", to: "2025-12-27" },
  };

  const result = BacktestManifestSchema.parse(manifest);
  assertEquals(result.date_range?.from, "2025-12-20");
});

Deno.test("BacktestManifestSchema rejects manifest without dates or date_range", () => {
  const manifest = {
    feed: "kalshi",
  };

  assertThrows(() => BacktestManifestSchema.parse(manifest));
});

Deno.test("BacktestResultSchema validates full result", () => {
  const result = {
    run_id: "backtest-abc123",
    signal: {
      id: "spread-alert",
      path: "signals/spread-alert/signal.ts",
      git_sha: "a1b2c3d",
      dirty: false,
    },
    data: {
      feed: "kalshi",
      dates: ["2025-12-25"],
      records_processed: 1000,
      tickers_seen: 50,
      data_fingerprint: "sha256:abc123",
    },
    results: {
      fire_count: 5,
      fire_rate: 0.005,
      fires: [],
    },
    execution: {
      started_at: "2025-12-28T15:00:00Z",
      completed_at: "2025-12-28T15:01:00Z",
      duration_ms: 60000,
      worker_id: "worker-1",
    },
  };

  const parsed = BacktestResultSchema.parse(result);
  assertEquals(parsed.run_id, "backtest-abc123");
  assertEquals(parsed.signal.id, "spread-alert");
  assertEquals(parsed.data.records_processed, 1000);
  assertEquals(parsed.results.fire_count, 5);
});

Deno.test("BacktestResultSchema validates result with fires", () => {
  const result = {
    run_id: "backtest-xyz789",
    signal: {
      id: "test-signal",
      path: "signals/test/signal.ts",
      git_sha: "abc1234",
      dirty: true,
    },
    data: {
      feed: "kalshi",
      dates: ["2025-12-25"],
      records_processed: 500,
      tickers_seen: 10,
      data_fingerprint: "sha256:def456",
    },
    results: {
      fire_count: 2,
      fire_rate: 0.004,
      fires: [
        { time: "2025-12-25T10:30:00Z", ticker: "TICKER-A", payload: { spread: 7.5 } },
        { time: "2025-12-25T14:15:00Z", ticker: "TICKER-B", payload: { spread: 8.2 } },
      ],
    },
    execution: {
      started_at: "2025-12-28T16:00:00Z",
      completed_at: "2025-12-28T16:00:30Z",
      duration_ms: 30000,
      worker_id: "worker-2",
    },
  };

  const parsed = BacktestResultSchema.parse(result);
  assertEquals(parsed.results.fires.length, 2);
  assertEquals(parsed.results.fires[0].ticker, "TICKER-A");
});
