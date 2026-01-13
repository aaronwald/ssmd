// ssmd-notifier/test/config.test.ts
import { assertEquals, assertThrows } from "@std/assert";
import { parseDestinations } from "../src/config.ts";

Deno.test("parseDestinations - parses valid JSON", () => {
  const json = JSON.stringify([
    { name: "test", type: "ntfy", config: { topic: "alerts" } },
  ]);
  const dests = parseDestinations(json);
  assertEquals(dests.length, 1);
  assertEquals(dests[0].name, "test");
});

Deno.test("parseDestinations - throws on invalid JSON", () => {
  assertThrows(
    () => parseDestinations("not json"),
    Error,
    "parse"
  );
});

Deno.test("parseDestinations - throws on non-array", () => {
  assertThrows(
    () => parseDestinations("{}"),
    Error,
    "array"
  );
});
