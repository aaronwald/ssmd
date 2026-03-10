import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { validateUrl } from "./http.ts";
import { HTTP_URL_ALLOWLIST } from "../types.ts";

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
