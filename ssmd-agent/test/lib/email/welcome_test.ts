import { assert, assertEquals, assertStringIncludes } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { composeWelcomeEmail } from "../../../src/lib/email/welcome.ts";

Deno.test("composeWelcomeEmail includes the link and metadata, never the raw secret", () => {
  const { subject, text } = composeWelcomeEmail({
    recipient: "billiron@gmail.com",
    link: "https://onetimesecret.com/secret/abc",
    apiBaseUrl: "https://api.varshtat.com",
    feeds: ["hols", "kalshi"],
    dateFrom: "2026-01-01",
    dateTo: "2099-12-31",
    ttlDays: 7,
    rawSecret: "sk_live_SHOULD_NOT_APPEAR",
  });
  assertStringIncludes(text, "https://onetimesecret.com/secret/abc");
  assertStringIncludes(text, "hols");
  assertStringIncludes(text, "Crypto OHLCV bars");
  assertStringIncludes(text, "api.varshtat.com");
  assertStringIncludes(text, "7 day");
  assertStringIncludes(text, "https://harman.varshtat.com");
  assert(!text.includes("sk_live_SHOULD_NOT_APPEAR"), "raw secret must never be in the email");
  assertEquals(typeof subject, "string");
});
