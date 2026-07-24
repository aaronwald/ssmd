import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  cutoffDate,
  datesToDrop,
  LIFECYCLE_MESSAGE_TYPE,
  manifestCoversLifecycle,
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

/** Build a minimal ParquetManifest `totals` for the lifecycle message type. */
function manifest(totals: Partial<ParquetManifest["totals"]>): ParquetManifest {
  return { totals } as unknown as ParquetManifest;
}

Deno.test("manifestCoversLifecycle: null manifest is not covered", () => {
  assertEquals(manifestCoversLifecycle(null), false);
});

Deno.test("manifestCoversLifecycle: manifest with no totals is not covered", () => {
  assertEquals(manifestCoversLifecycle({} as unknown as ParquetManifest), false);
});

Deno.test("manifestCoversLifecycle: zero lifecycle rows written is not covered", () => {
  assertEquals(
    manifestCoversLifecycle(manifest({ records_written: {} })),
    false,
  );
});

Deno.test("manifestCoversLifecycle: rows for a DIFFERENT message type do not cover lifecycle", () => {
  assertEquals(
    manifestCoversLifecycle(manifest({ records_written: { some_other_type: 42 } })),
    false,
  );
});

Deno.test("manifestCoversLifecycle: clean full archive is covered", () => {
  assertEquals(
    manifestCoversLifecycle(manifest({
      records_written: { [LIFECYCLE_MESSAGE_TYPE]: 1000 },
      parse_batch_dropped: {},
      lines_json_error: 0,
      lines_type_unknown: 0,
    })),
    true,
  );
});

Deno.test("manifestCoversLifecycle: partial archive (dropped lifecycle records) is NOT covered", () => {
  // The trap: 5 of 1000 written, 995 dropped. records_written>0 but must still be rejected.
  assertEquals(
    manifestCoversLifecycle(manifest({
      records_written: { [LIFECYCLE_MESSAGE_TYPE]: 5 },
      parse_batch_dropped: { [LIFECYCLE_MESSAGE_TYPE]: 995 },
    })),
    false,
  );
});

Deno.test("manifestCoversLifecycle: stream-level parse errors block coverage", () => {
  assertEquals(
    manifestCoversLifecycle(manifest({
      records_written: { [LIFECYCLE_MESSAGE_TYPE]: 1000 },
      lines_json_error: 3,
    })),
    false,
  );
  assertEquals(
    manifestCoversLifecycle(manifest({
      records_written: { [LIFECYCLE_MESSAGE_TYPE]: 1000 },
      lines_type_unknown: 1,
    })),
    false,
  );
});

Deno.test("manifestCoversLifecycle: any lines_no_schema entry blocks coverage", () => {
  assertEquals(
    manifestCoversLifecycle(manifest({
      records_written: { [LIFECYCLE_MESSAGE_TYPE]: 1000 },
      lines_no_schema: { market_lifecycle_v2: 7 },
    })),
    false,
  );
});
