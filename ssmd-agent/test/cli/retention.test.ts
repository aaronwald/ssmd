import { assertEquals, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { runLifecycleRetention } from "../../src/cli/commands/retention.ts";

Deno.test("runLifecycleRetention: deletes every non-empty covered date", async () => {
  const deleted: string[] = [];
  const res = await runLifecycleRetention({
    now: new Date("2026-07-24T00:00:00Z"),
    retentionDays: 30,
    oldest: "2026-06-22",
    count: () => Promise.resolve(10),
    check: () => Promise.resolve(true),
    del: (d) => { deleted.push(d); return Promise.resolve(10); },
  });
  assertEquals(deleted, ["2026-06-22", "2026-06-23"]); // cutoff = 2026-06-24, exclusive
  assertEquals(res.dropped, 20);
  assertEquals(res.datesSkipped, []);
});

Deno.test("runLifecycleRetention: gate failure stops deletes at the uncovered date", async () => {
  const deleted: string[] = [];
  const checked: string[] = [];
  await assertRejects(
    () => runLifecycleRetention({
      now: new Date("2026-07-24T00:00:00Z"),
      retentionDays: 30,
      oldest: "2026-06-21",
      count: () => Promise.resolve(10),
      check: (d) => { checked.push(d); return Promise.resolve(d !== "2026-06-22"); }, // 22nd not archived
      del: (d) => { deleted.push(d); return Promise.resolve(10); },
    }),
    Error,
    "2026-06-22 not completely archived",
  );
  // only the covered 21st was deleted; 22nd (uncovered) and 23rd were NOT
  assertEquals(deleted, ["2026-06-21"]);
  // the loop short-circuits: 23rd is never even checked
  assertEquals(checked, ["2026-06-21", "2026-06-22"]);
});

Deno.test("runLifecycleRetention: empty days are skipped without gating (no wedge)", async () => {
  const deleted: string[] = [];
  const checked: string[] = [];
  // 2026-06-22 has no rows AND no archive manifest — it must be skipped, not block the run.
  const res = await runLifecycleRetention({
    now: new Date("2026-07-24T00:00:00Z"),
    retentionDays: 30,
    oldest: "2026-06-21",
    count: (d) => Promise.resolve(d === "2026-06-22" ? 0 : 5),
    check: (d) => { checked.push(d); return Promise.resolve(true); },
    del: (d) => { deleted.push(d); return Promise.resolve(5); },
  });
  // 21st and 23rd deleted; 22nd skipped and never gated
  assertEquals(deleted, ["2026-06-21", "2026-06-23"]);
  assertEquals(res.datesSkipped, ["2026-06-22"]);
  assertEquals(checked, ["2026-06-21", "2026-06-23"]);
  assertEquals(res.dropped, 10);
});

Deno.test("runLifecycleRetention: no-op when table empty (oldest null)", async () => {
  const res = await runLifecycleRetention({
    now: new Date("2026-07-24T00:00:00Z"),
    retentionDays: 30,
    oldest: null,
    count: () => Promise.resolve(0),
    check: () => Promise.resolve(true),
    del: () => Promise.resolve(0),
  });
  assertEquals(res.dropped, 0);
  assertEquals(res.datesDropped, []);
  assertEquals(res.datesSkipped, []);
});
