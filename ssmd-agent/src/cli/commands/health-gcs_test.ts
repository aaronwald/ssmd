import { assert, assertEquals, assertThrows } from "jsr:@std/assert";
import { parseGcsFileInfo, resolveScanDate } from "./health-gcs.ts";

Deno.test("parseGcsFileInfo extracts msgType + HHMM from a parquet object name", () => {
  const f = parseGcsFileInfo("ticker_1400.parquet", 12345);
  assert(f !== null, "expected a parsed GcsFileInfo, got null");
  assertEquals(f.msgType, "ticker");
  assertEquals(f.time, "1400");
  assertEquals(f.sizeBytes, 12345);
  assertEquals(f.name, "ticker_1400.parquet");
});

Deno.test("parseGcsFileInfo handles msgType with underscores (last underscore splits)", () => {
  const f = parseGcsFileInfo("order_book_0930.parquet", 7);
  assert(f !== null, "expected a parsed GcsFileInfo");
  assertEquals(f.msgType, "order_book");
  assertEquals(f.time, "0930");
});

Deno.test("parseGcsFileInfo rejects wrong ext", () => {
  assertEquals(parseGcsFileInfo("manifest.json", 1), null);
  assertEquals(parseGcsFileInfo("ticker_1400.parquet", 1, "jsonl"), null);
});

Deno.test("parseGcsFileInfo rejects names without an underscore", () => {
  assertEquals(parseGcsFileInfo("nounderscore.parquet", 1), null);
});

Deno.test("resolveScanDate returns the previous UTC day by default", () => {
  assertEquals(resolveScanDate(new Date("2026-07-25T08:30:00Z")), "2026-07-24");
});

Deno.test("resolveScanDate crosses a month boundary", () => {
  assertEquals(resolveScanDate(new Date("2026-08-01T00:05:00Z")), "2026-07-31");
});

Deno.test("resolveScanDate crosses a year boundary", () => {
  assertEquals(resolveScanDate(new Date("2026-01-01T23:59:00Z")), "2025-12-31");
});

Deno.test("resolveScanDate uses UTC, not local time", () => {
  assertEquals(resolveScanDate(new Date("2026-07-25T00:30:00Z")), "2026-07-24");
  assertEquals(resolveScanDate(new Date("2026-07-25T23:30:00Z")), "2026-07-24");
});

Deno.test("resolveScanDate honours a valid override verbatim", () => {
  assertEquals(resolveScanDate(new Date("2026-07-25T08:30:00Z"), "2026-05-01"), "2026-05-01");
});

Deno.test("resolveScanDate rejects a malformed override", () => {
  assertThrows(() => resolveScanDate(new Date("2026-07-25T08:30:00Z"), "07/25/2026"));
  assertThrows(() => resolveScanDate(new Date("2026-07-25T08:30:00Z"), "2026-13-01"));
  assertThrows(() => resolveScanDate(new Date("2026-07-25T08:30:00Z"), "not-a-date"));
  assertThrows(() => resolveScanDate(new Date("2026-07-25T08:30:00Z"), "2026-02-30"));
  assertThrows(() => resolveScanDate(new Date("2026-07-25T08:30:00Z"), "2026-04-31"));
  assertThrows(() => resolveScanDate(new Date("2026-07-25T08:30:00Z"), "2026-02-29")); // non-leap
});

Deno.test("resolveScanDate accepts a real leap day", () => {
  assertEquals(resolveScanDate(new Date("2026-07-25T08:30:00Z"), "2024-02-29"), "2024-02-29");
});

Deno.test("resolveScanDate does not mutate the injected now", () => {
  const now = new Date("2026-07-25T08:30:00Z");
  resolveScanDate(now);
  assertEquals(now.toISOString(), "2026-07-25T08:30:00.000Z");
});
