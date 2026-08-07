import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  normalizePreOpenPages,
  printSyncSummary,
  type SyncOptions,
  type SyncResult,
} from "../../src/cli/commands/secmaster.ts";

Deno.test("SyncOptions types correctly", () => {
  const options: SyncOptions = {
    eventsOnly: true,
    marketsOnly: false,
    noDelete: false,
    dryRun: true,
  };

  assertEquals(options.eventsOnly, true);
  assertEquals(options.dryRun, true);
});

Deno.test("SyncOptions defaults to empty object", () => {
  const options: SyncOptions = {};

  assertEquals(options.eventsOnly, undefined);
  assertEquals(options.marketsOnly, undefined);
});

Deno.test("normalizePreOpenPages: undefined defaults to 1 (original single-page behavior)", () => {
  assertEquals(normalizePreOpenPages(undefined, "KXBTCD"), 1);
});

Deno.test("normalizePreOpenPages: valid values pass through", () => {
  assertEquals(normalizePreOpenPages(1, "KXBTC15M"), 1);
  assertEquals(normalizePreOpenPages(8, "KXBTC15M"), 8);
});

Deno.test("normalizePreOpenPages: non-integers floor down", () => {
  assertEquals(normalizePreOpenPages(8.9, "KXBTC15M"), 8);
});

Deno.test("normalizePreOpenPages: invalid values fall back to 1, never 0 or negative", () => {
  assertEquals(normalizePreOpenPages(0, "KXBTC15M"), 1);
  assertEquals(normalizePreOpenPages(-5, "KXBTC15M"), 1);
  assertEquals(normalizePreOpenPages(NaN, "KXBTC15M"), 1);
  assertEquals(normalizePreOpenPages(Infinity, "KXBTC15M"), 1);
});

Deno.test("normalizePreOpenPages: clamps to the safety cap so a bad flag can't crawl history", () => {
  assertEquals(normalizePreOpenPages(9999, "KXBTC15M"), 50);
});

Deno.test("SyncOptions accepts preOpenPages", () => {
  const options: SyncOptions = { bySeries: true, seriesSuffix: "15M", preOpenPages: 8 };
  assertEquals(options.preOpenPages, 8);
});

Deno.test("SyncResult structure is correct", () => {
  const result: SyncResult = {
    events: { fetched: 100, upserted: 95, deleted: 5, durationMs: 1000 },
    markets: { fetched: 500, upserted: 480, skipped: 10, deleted: 10, durationMs: 5000 },
    totalDurationMs: 6000,
  };

  assertEquals(result.events.fetched, 100);
  assertEquals(result.markets.skipped, 10);
  assertEquals(result.totalDurationMs, 6000);
});

Deno.test("printSyncSummary does not throw", () => {
  const result: SyncResult = {
    events: { fetched: 50, upserted: 50, deleted: 0, durationMs: 500 },
    markets: { fetched: 200, upserted: 195, skipped: 5, deleted: 0, durationMs: 2000 },
    totalDurationMs: 2500,
  };

  // Should not throw
  printSyncSummary(result);
});

Deno.test("printSyncSummary handles zero values", () => {
  const result: SyncResult = {
    events: { fetched: 0, upserted: 0, deleted: 0, durationMs: 0 },
    markets: { fetched: 0, upserted: 0, skipped: 0, deleted: 0, durationMs: 0 },
    totalDurationMs: 0,
  };

  // Should not throw
  printSyncSummary(result);
});
