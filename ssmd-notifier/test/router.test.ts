// ssmd-notifier/test/router.test.ts
import { assertEquals } from "@std/assert";
import { matches, shouldRoute } from "../src/router.ts";
import type { SignalFire, MatchRule, Destination } from "../src/types.ts";

const fire: SignalFire = {
  signalId: "volume-spike",
  ts: 1704067200,
  ticker: "GOOGL-250117-W185",
  payload: { dollarVolume: 15234 },
};

Deno.test("matches - eq operator matches exact value", () => {
  const rule: MatchRule = { field: "signalId", operator: "eq", value: "volume-spike" };
  assertEquals(matches(fire, rule), true);
});

Deno.test("matches - eq operator rejects different value", () => {
  const rule: MatchRule = { field: "signalId", operator: "eq", value: "other-signal" };
  assertEquals(matches(fire, rule), false);
});

Deno.test("matches - contains operator matches substring", () => {
  const rule: MatchRule = { field: "signalId", operator: "contains", value: "volume" };
  assertEquals(matches(fire, rule), true);
});

Deno.test("matches - contains operator rejects non-substring", () => {
  const rule: MatchRule = { field: "signalId", operator: "contains", value: "momentum" };
  assertEquals(matches(fire, rule), false);
});

Deno.test("matches - ticker field works", () => {
  const rule: MatchRule = { field: "ticker", operator: "contains", value: "GOOGL" };
  assertEquals(matches(fire, rule), true);
});

Deno.test("matches - unknown field returns false", () => {
  const rule: MatchRule = { field: "unknown", operator: "eq", value: "test" };
  assertEquals(matches(fire, rule), false);
});

Deno.test("shouldRoute - no match rule routes all fires", () => {
  const dest: Destination = {
    name: "all",
    type: "ntfy",
    config: { topic: "test" },
  };
  assertEquals(shouldRoute(fire, dest), true);
});

Deno.test("shouldRoute - matching rule routes fire", () => {
  const dest: Destination = {
    name: "volume-only",
    type: "ntfy",
    config: { topic: "test" },
    match: { field: "signalId", operator: "contains", value: "volume" },
  };
  assertEquals(shouldRoute(fire, dest), true);
});

Deno.test("shouldRoute - non-matching rule blocks fire", () => {
  const dest: Destination = {
    name: "momentum-only",
    type: "ntfy",
    config: { topic: "test" },
    match: { field: "signalId", operator: "eq", value: "momentum" },
  };
  assertEquals(shouldRoute(fire, dest), false);
});
