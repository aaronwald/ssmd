import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { stripForbiddenHeaders, validateUrl } from "./http.ts";
import { FORBIDDEN_HEADER_KEYS, HTTP_URL_ALLOWLIST } from "../types.ts";

Deno.test("validateUrl: allows data-ts internal", () => {
  assertEquals(validateUrl("http://ssmd-data-ts-internal:8081/v1/health", HTTP_URL_ALLOWLIST), true);
});

Deno.test("validateUrl: rejects external URL", () => {
  assertEquals(validateUrl("https://evil.com/steal", HTTP_URL_ALLOWLIST), false);
});

Deno.test("validateUrl: rejects localhost", () => {
  assertEquals(validateUrl("http://localhost:5432/", HTTP_URL_ALLOWLIST), false);
});

Deno.test("validateUrl: rejects metadata server", () => {
  assertEquals(validateUrl("http://169.254.169.254/metadata", HTTP_URL_ALLOWLIST), false);
});

Deno.test("validateUrl: rejects redis", () => {
  assertEquals(validateUrl("http://ssmd-redis:6379/", HTTP_URL_ALLOWLIST), false);
});

Deno.test("validateUrl: rejects userinfo bypass", () => {
  assertEquals(validateUrl("http://evil@ssmd-data-ts-internal:8081/", HTTP_URL_ALLOWLIST), false);
});

Deno.test("validateUrl: rejects invalid URL", () => {
  assertEquals(validateUrl("not-a-url", HTTP_URL_ALLOWLIST), false);
});

Deno.test("validateUrl: allows path under allowed host", () => {
  assertEquals(validateUrl("http://ssmd-data-ts-internal:8081/v1/data/trades?feed=kalshi", HTTP_URL_ALLOWLIST), true);
});

Deno.test("stripForbiddenHeaders: removes authorization header (case-insensitive)", () => {
  const headers = { "Authorization": "Bearer secret", "Content-Type": "application/json" };
  const result = stripForbiddenHeaders(headers);
  assertEquals(result, { "Content-Type": "application/json" });
});

Deno.test("stripForbiddenHeaders: removes cookie and x-api-key", () => {
  const headers = { "Cookie": "session=abc", "X-API-Key": "sk_live_123", "Accept": "*/*" };
  const result = stripForbiddenHeaders(headers);
  assertEquals(result, { "Accept": "*/*" });
});

Deno.test("stripForbiddenHeaders: removes x-api-token", () => {
  const headers = { "x-api-token": "tok_abc" };
  const result = stripForbiddenHeaders(headers);
  assertEquals(result, {});
});

Deno.test("stripForbiddenHeaders: no-op on safe headers", () => {
  const headers = { "Content-Type": "application/json", "Accept": "*/*" };
  const result = stripForbiddenHeaders(headers);
  assertEquals(result, headers);
});

Deno.test("FORBIDDEN_HEADER_KEYS: contains expected keys", () => {
  assertEquals(FORBIDDEN_HEADER_KEYS.includes("authorization"), true);
  assertEquals(FORBIDDEN_HEADER_KEYS.includes("cookie"), true);
  assertEquals(FORBIDDEN_HEADER_KEYS.includes("x-api-key"), true);
  assertEquals(FORBIDDEN_HEADER_KEYS.includes("x-api-token"), true);
});
