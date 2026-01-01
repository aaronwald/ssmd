import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { detectPII, redactPII } from "../../../src/lib/guardrails/pii.ts";

Deno.test("detectPII finds email addresses", () => {
  const result = detectPII("Contact me at john@example.com");
  assertEquals(result.length, 1);
  assertEquals(result[0].type, "email");
});

Deno.test("detectPII finds credit card numbers", () => {
  const result = detectPII("My card is 4111-1111-1111-1111");
  assertEquals(result.length, 1);
  assertEquals(result[0].type, "credit_card");
});

Deno.test("detectPII finds SSN", () => {
  const result = detectPII("SSN: 123-45-6789");
  assertEquals(result.length, 1);
  assertEquals(result[0].type, "ssn");
});

Deno.test("detectPII finds phone numbers", () => {
  const result = detectPII("Call me at (555) 123-4567");
  assertEquals(result.length, 1);
  assertEquals(result[0].type, "phone");
});

Deno.test("redactPII replaces PII with placeholders", () => {
  const input = "Email john@example.com or call 555-123-4567";
  const result = redactPII(input);
  assertEquals(result.includes("john@example.com"), false);
  assertEquals(result.includes("[REDACTED_EMAIL]"), true);
});
