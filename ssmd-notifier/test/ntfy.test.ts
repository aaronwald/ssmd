// ssmd-notifier/test/ntfy.test.ts
import { assertEquals, assertStringIncludes } from "@std/assert";
import { NtfySender } from "../src/senders/ntfy.ts";
import type { SignalFire, Destination } from "../src/types.ts";

const fire: SignalFire = {
  signalId: "volume-spike",
  ts: 1704067200,
  ticker: "GOOGL-250117-W185",
  payload: { dollarVolume: 15234 },
};

Deno.test("NtfySender - formats title correctly", () => {
  const sender = new NtfySender();
  const title = sender.formatTitle(fire);
  assertStringIncludes(title, "volume-spike");
  assertStringIncludes(title, "GOOGL-250117-W185");
});

Deno.test("NtfySender - formats body as JSON", () => {
  const sender = new NtfySender();
  const body = sender.formatBody(fire);
  assertStringIncludes(body, "dollarVolume");
  assertStringIncludes(body, "15234");
});

Deno.test("NtfySender - builds correct URL", () => {
  const sender = new NtfySender();
  const dest: Destination = {
    name: "test",
    type: "ntfy",
    config: { server: "https://ntfy.example.com", topic: "alerts" },
  };
  const url = sender.buildUrl(dest);
  assertEquals(url, "https://ntfy.example.com/alerts");
});

Deno.test("NtfySender - uses default server", () => {
  const sender = new NtfySender();
  const dest: Destination = {
    name: "test",
    type: "ntfy",
    config: { topic: "alerts" },
  };
  const url = sender.buildUrl(dest);
  assertEquals(url, "https://ntfy.sh/alerts");
});
