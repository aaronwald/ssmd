import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { kxbtcdCanary } from "./kxbtcd-canary.ts";
import type { CodeInput } from "./mod.ts";

function makeSqlOutput(rows: Array<{
  event_ticker: string;
  active_markets: number;
  total_markets: number;
  earliest_close: string;
  latest_close: string;
}>): string {
  return JSON.stringify({ rows, row_count: rows.length });
}

function makeHttpOutput(events: Array<{ ticker: string; [key: string]: unknown }>): string {
  return JSON.stringify({ status: 200, body: { events }, truncated: false });
}

function makeInput(
  sqlRows: Array<{
    event_ticker: string;
    active_markets: number;
    total_markets: number;
    earliest_close: string;
    latest_close: string;
  }>,
  redisEvents: Array<{ ticker: string; [key: string]: unknown }>,
  params?: Record<string, unknown>,
): CodeInput {
  return {
    stages: {
      0: { output: makeSqlOutput(sqlRows) },
      1: { output: makeHttpOutput(redisEvents) },
    },
    triggerInfo: {},
    date: "2026-03-13",
    params,
  };
}

function makeHealthyRows(count: number) {
  return Array.from({ length: count }, (_, i) => ({
    event_ticker: `KXBTCD-26MAR13${String(i).padStart(2, "0")}`,
    active_markets: 2,
    total_markets: 2,
    earliest_close: "2026-03-13T16:00:00Z",
    latest_close: "2026-03-13T17:00:00Z",
  }));
}

function makeHealthyRedisEvents(count: number) {
  return Array.from({ length: count }, (_, i) => ({
    ticker: `KXBTCD-26MAR13${String(i).padStart(2, "0")}`,
    status: "active",
  }));
}

Deno.test("kxbtcd-canary: all healthy — skip", () => {
  const rows = makeHealthyRows(20);
  const events = makeHealthyRedisEvents(20);
  const result = kxbtcdCanary(makeInput(rows, events));

  assertEquals(result.skip, true);
  const r = result.result as Record<string, unknown>;
  assertEquals(r.healthy, true);
  assertEquals(r.dbEventCount, 20);
  assertEquals(r.dbActiveEventCount, 20);
  assertEquals(r.redisEventCount, 20);
  assertEquals((r.issues as string[]).length, 0);
});

Deno.test("kxbtcd-canary: too few active events — no skip", () => {
  const rows = makeHealthyRows(5);
  const events = makeHealthyRedisEvents(5);
  const result = kxbtcdCanary(makeInput(rows, events));

  assertEquals(result.skip, false);
  const r = result.result as Record<string, unknown>;
  assertEquals(r.healthy, false);
  const issues = r.issues as string[];
  assertEquals(issues.length, 1);
  assertEquals(issues[0].includes("only 5 active"), true);
});

Deno.test("kxbtcd-canary: custom minActiveEvents threshold", () => {
  const rows = makeHealthyRows(5);
  const events = makeHealthyRedisEvents(5);
  const result = kxbtcdCanary(makeInput(rows, events, { minActiveEvents: 3 }));

  assertEquals(result.skip, true);
  assertEquals((result.result as Record<string, unknown>).healthy, true);
});

Deno.test("kxbtcd-canary: zero active markets bug — no skip", () => {
  const rows = [
    ...makeHealthyRows(15),
    {
      event_ticker: "KXBTCD-26MAR1316",
      active_markets: 0,
      total_markets: 2,
      earliest_close: "2026-03-13T20:00:00Z",
      latest_close: "2026-03-13T21:00:00Z",
    },
  ];
  const events = [...makeHealthyRedisEvents(15), { ticker: "KXBTCD-26MAR1316", status: "active" }];
  const result = kxbtcdCanary(makeInput(rows, events));

  assertEquals(result.skip, false);
  const r = result.result as Record<string, unknown>;
  assertEquals(r.healthy, false);
  const issues = r.issues as string[];
  assertEquals(issues.some((i: string) => i.includes("0 active markets")), true);
  assertEquals((r.zeroActiveEvents as string[]).includes("KXBTCD-26MAR1316"), true);
});

Deno.test("kxbtcd-canary: missing from Redis — no skip", () => {
  const rows = makeHealthyRows(15);
  // Redis only has 12 of the 15
  const events = makeHealthyRedisEvents(12);
  const result = kxbtcdCanary(makeInput(rows, events));

  assertEquals(result.skip, false);
  const r = result.result as Record<string, unknown>;
  assertEquals(r.healthy, false);
  const issues = r.issues as string[];
  assertEquals(issues.some((i: string) => i.includes("missing from Redis")), true);
  assertEquals((r.missingFromRedis as string[]).length, 3);
});

Deno.test("kxbtcd-canary: extra events in Redis — still healthy", () => {
  const rows = makeHealthyRows(15);
  const events = [
    ...makeHealthyRedisEvents(15),
    { ticker: "KXBTCD-26MAR1299", status: "active" }, // stale cache entry
  ];
  const result = kxbtcdCanary(makeInput(rows, events));

  assertEquals(result.skip, true);
  const r = result.result as Record<string, unknown>;
  assertEquals(r.healthy, true);
  assertEquals((r.extraInRedis as string[]).length, 1);
});

Deno.test("kxbtcd-canary: no SQL output — error", () => {
  const input: CodeInput = {
    stages: { 1: { output: makeHttpOutput([]) } },
    triggerInfo: {},
    date: "2026-03-13",
  };
  const result = kxbtcdCanary(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "No SQL stage output found");
});

Deno.test("kxbtcd-canary: no HTTP output — error", () => {
  const input: CodeInput = {
    stages: { 0: { output: makeSqlOutput(makeHealthyRows(15)) } },
    triggerInfo: {},
    date: "2026-03-13",
  };
  const result = kxbtcdCanary(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "No HTTP stage output found");
});

Deno.test("kxbtcd-canary: malformed SQL output — error", () => {
  const input: CodeInput = {
    stages: {
      0: { output: "not json" },
      1: { output: makeHttpOutput([]) },
    },
    triggerInfo: {},
    date: "2026-03-13",
  };
  const result = kxbtcdCanary(input);
  assertEquals(result.skip, false);
  assertEquals((result.result as Record<string, unknown>).error, "Failed to parse SQL stage output");
});

Deno.test("kxbtcd-canary: combined issues — all flagged", () => {
  // Only 8 events, 1 has zero active, and 2 missing from Redis
  const rows = [
    ...makeHealthyRows(7),
    {
      event_ticker: "KXBTCD-26MAR1307",
      active_markets: 0,
      total_markets: 2,
      earliest_close: "2026-03-13T11:00:00Z",
      latest_close: "2026-03-13T12:00:00Z",
    },
  ];
  const events = makeHealthyRedisEvents(6); // only 6 of 8 in Redis
  const result = kxbtcdCanary(makeInput(rows, events));

  assertEquals(result.skip, false);
  const r = result.result as Record<string, unknown>;
  const issues = r.issues as string[];
  // Should have 3 issues: too few active (7 < 10), zero active bug, missing from Redis
  assertEquals(issues.length, 3);
  assertEquals(issues.some((i: string) => i.includes("only 7 active")), true);
  assertEquals(issues.some((i: string) => i.includes("0 active markets")), true);
  assertEquals(issues.some((i: string) => i.includes("missing from Redis")), true);
});
