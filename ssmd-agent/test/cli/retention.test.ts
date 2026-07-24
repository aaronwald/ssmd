import { assertEquals, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { runLifecycleRetention } from "../../src/cli/commands/retention.ts";

Deno.test("runLifecycleRetention: deletes every date when all covered", async () => {
  const deleted: string[] = [];
  const res = await runLifecycleRetention({
    now: new Date("2026-07-24T00:00:00Z"),
    retentionDays: 30,
    oldest: "2026-06-22",
    check: () => Promise.resolve(true),
    del: (d) => { deleted.push(d); return Promise.resolve(1); },
  });
  assertEquals(deleted, ["2026-06-22", "2026-06-23"]); // cutoff = 2026-06-24, exclusive
  assertEquals(res.dropped, 2);
});

Deno.test("runLifecycleRetention: gate failure stops deletes at the uncovered date", async () => {
  const deleted: string[] = [];
  await assertRejects(
    () => runLifecycleRetention({
      now: new Date("2026-07-24T00:00:00Z"),
      retentionDays: 30,
      oldest: "2026-06-21",
      check: (d) => Promise.resolve(d !== "2026-06-22"), // 22nd not archived
      del: (d) => { deleted.push(d); return Promise.resolve(1); },
    }),
    Error,
    "2026-06-22 not archived",
  );
  // only the covered 21st was deleted; 22nd (uncovered) and 23rd were NOT
  assertEquals(deleted, ["2026-06-21"]);
});

Deno.test("runLifecycleRetention: no-op when table empty (oldest null)", async () => {
  const res = await runLifecycleRetention({
    now: new Date("2026-07-24T00:00:00Z"),
    retentionDays: 30,
    oldest: null,
    check: () => Promise.resolve(true),
    del: () => Promise.resolve(0),
  });
  assertEquals(res.dropped, 0);
  assertEquals(res.datesDropped, []);
});
