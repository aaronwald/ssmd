// test/cli/signal-deploy.test.ts
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { formatAge } from "../../src/cli/commands/signal-deploy.ts";

Deno.test("formatAge - formats seconds", () => {
  const now = new Date();
  const thirtySecsAgo = new Date(now.getTime() - 30 * 1000);
  const result = formatAge(thirtySecsAgo.toISOString());
  assertEquals(result, "30s");
});

Deno.test("formatAge - formats minutes", () => {
  const now = new Date();
  const fiveMinAgo = new Date(now.getTime() - 5 * 60 * 1000);
  const result = formatAge(fiveMinAgo.toISOString());
  assertEquals(result, "5m");
});

Deno.test("formatAge - formats hours", () => {
  const now = new Date();
  const threeHoursAgo = new Date(now.getTime() - 3 * 60 * 60 * 1000);
  const result = formatAge(threeHoursAgo.toISOString());
  assertEquals(result, "3h");
});

Deno.test("formatAge - formats days", () => {
  const now = new Date();
  const twoDaysAgo = new Date(now.getTime() - 2 * 24 * 60 * 60 * 1000);
  const result = formatAge(twoDaysAgo.toISOString());
  assertEquals(result, "2d");
});

Deno.test("formatAge - prefers days over hours for multi-day spans", () => {
  const now = new Date();
  const fiveDaysAgo = new Date(now.getTime() - 5 * 24 * 60 * 60 * 1000);
  const result = formatAge(fiveDaysAgo.toISOString());
  assertEquals(result, "5d");
});
