// ssmd-agent/test/cli-utils.test.ts
import { assertEquals } from "jsr:@std/assert";
import { formatArgs, formatResult } from "../src/cli-utils.ts";

Deno.test("formatArgs - formats object as key=value pairs", () => {
  const input = { market: "BTCUSD", limit: 10 };
  const result = formatArgs(input);
  assertEquals(result, 'market="BTCUSD", limit=10');
});

Deno.test("formatArgs - filters undefined values", () => {
  const input = { market: "BTCUSD", limit: undefined };
  const result = formatArgs(input);
  assertEquals(result, 'market="BTCUSD"');
});

Deno.test("formatArgs - returns string representation for non-objects", () => {
  assertEquals(formatArgs("hello"), "hello");
  assertEquals(formatArgs(123), "123");
  assertEquals(formatArgs(null), "null");
});

Deno.test("formatArgs - handles empty object", () => {
  assertEquals(formatArgs({}), "");
});

Deno.test("formatResult - returns item count for arrays", () => {
  const result = formatResult(JSON.stringify([1, 2, 3, 4, 5]));
  assertEquals(result, "5 items");
});

Deno.test("formatResult - returns snapshot count", () => {
  const result = formatResult(JSON.stringify({ count: 42 }));
  assertEquals(result, "42 snapshots");
});

Deno.test("formatResult - returns fires and errors count", () => {
  const result = formatResult(JSON.stringify({ fires: 10, errors: ["err1", "err2"] }));
  assertEquals(result, "10 fires, 2 errors");
});

Deno.test("formatResult - returns fires with zero errors", () => {
  const result = formatResult(JSON.stringify({ fires: 5 }));
  assertEquals(result, "5 fires, 0 errors");
});

Deno.test("formatResult - returns commit sha", () => {
  const result = formatResult(JSON.stringify({ sha: "abc123def" }));
  assertEquals(result, "Committed: abc123def");
});

Deno.test("formatResult - truncates long strings", () => {
  const longString = "a".repeat(150);
  const result = formatResult(longString);
  assertEquals(result.length, 103); // 100 chars + "..."
  assertEquals(result.endsWith("..."), true);
});

Deno.test("formatResult - does not truncate short strings", () => {
  const shortString = "hello world";
  const result = formatResult(shortString);
  assertEquals(result, "hello world");
});

Deno.test("formatResult - handles invalid JSON by truncating", () => {
  const invalidJson = "{not valid json}";
  const result = formatResult(invalidJson);
  assertEquals(result, "{not valid json}");
});

Deno.test("formatResult - handles non-string values", () => {
  assertEquals(formatResult(123), "123");
  assertEquals(formatResult(null), "null");
  assertEquals(formatResult(undefined), "undefined");
});

Deno.test("formatResult - truncates long JSON that doesn't match patterns", () => {
  const longJson = JSON.stringify({ data: "x".repeat(150) });
  const result = formatResult(longJson);
  assertEquals(result.length, 103);
  assertEquals(result.endsWith("..."), true);
});
