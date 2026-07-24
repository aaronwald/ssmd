import { assertEquals, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  assertDatesArchived,
  cutoffDate,
  datesToDrop,
  manifestHasRows,
  type DateArchivedCheck,
} from "../../../src/lib/retention/lifecycle.ts";
import type { ParquetManifest } from "../../../src/lib/gcs/catalog.ts";

Deno.test("cutoffDate: 30 days before a fixed UTC now", () => {
  assertEquals(cutoffDate(new Date("2026-07-24T09:00:00Z"), 30), "2026-06-24");
});

Deno.test("datesToDrop: inclusive from oldest, excludes cutoff", () => {
  assertEquals(datesToDrop("2026-06-22", "2026-06-24"), ["2026-06-22", "2026-06-23"]);
});

Deno.test("datesToDrop: empty when oldest is already inside the window", () => {
  assertEquals(datesToDrop("2026-06-24", "2026-06-24"), []);
  assertEquals(datesToDrop("2026-07-01", "2026-06-24"), []);
});

Deno.test("manifestHasRows: null manifest is not covered", () => {
  assertEquals(manifestHasRows(null), false);
});

Deno.test("manifestHasRows: manifest with zero rows is not covered", () => {
  const m = { totals: { records_written: {} } } as unknown as ParquetManifest;
  assertEquals(manifestHasRows(m), false);
});

Deno.test("manifestHasRows: manifest with written rows is covered", () => {
  const m = { totals: { records_written: { market_lifecycle_v2: 12 } } } as unknown as ParquetManifest;
  assertEquals(manifestHasRows(m), true);
});

Deno.test("assertDatesArchived: resolves when all dates covered", async () => {
  const check: DateArchivedCheck = () => Promise.resolve(true);
  await assertDatesArchived(["2026-06-22", "2026-06-23"], check);
});

Deno.test("assertDatesArchived: throws on first uncovered date", async () => {
  const seen: string[] = [];
  const check: DateArchivedCheck = (d) => {
    seen.push(d);
    return Promise.resolve(d !== "2026-06-23");
  };
  await assertRejects(
    () => assertDatesArchived(["2026-06-22", "2026-06-23", "2026-06-24"], check),
    Error,
    "2026-06-23 not archived",
  );
  assertEquals(seen, ["2026-06-22", "2026-06-23"]);
});
