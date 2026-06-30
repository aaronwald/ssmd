import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import versions from "../../src/server/schema-versions.json" with { type: "json" };

Deno.test("binance trade schema version mirrors the Rust 1.1.0 bump", () => {
  assertEquals(versions.binance.trade.version, "1.1.0");
});
