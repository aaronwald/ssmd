import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  validateToolCall,
  isTradingTool,
  TRADING_TOOLS,
} from "../../../../src/agent/middleware/validators/tool-rules.ts";

Deno.test("isTradingTool - identifies trading tools", () => {
  assertEquals(isTradingTool("place_order"), true);
  assertEquals(isTradingTool("cancel_order"), true);
  assertEquals(isTradingTool("modify_position"), true);
});

Deno.test("isTradingTool - non-trading tools return false", () => {
  assertEquals(isTradingTool("get_markets"), false);
  assertEquals(isTradingTool("get_fee_schedule"), false);
  assertEquals(isTradingTool("search_markets"), false);
});

Deno.test("validateToolCall - allows safe tools", () => {
  const result = validateToolCall({ name: "get_markets", args: {} });
  assertEquals(result.allowed, true);
});

Deno.test("validateToolCall - allows agent tools", () => {
  // Data exploration
  assertEquals(validateToolCall({ name: "list_datasets", args: {} }).allowed, true);
  assertEquals(validateToolCall({ name: "get_today", args: {} }).allowed, true);
  // State builders
  assertEquals(validateToolCall({ name: "orderbook_builder", args: {} }).allowed, true);
  // Market data
  assertEquals(validateToolCall({ name: "list_markets", args: {} }).allowed, true);
});

Deno.test("validateToolCall - flags trading tools for approval", () => {
  const result = validateToolCall({ name: "place_order", args: { ticker: "TEST" } });
  assertEquals(result.allowed, false);
  assertEquals(result.requiresApproval, true);
});

Deno.test("validateToolCall - rejects unknown tools", () => {
  const result = validateToolCall({ name: "unknown_dangerous_tool", args: {} });
  assertEquals(result.allowed, false);
  assertEquals(result.reason?.includes("unknown"), true);
});

Deno.test("TRADING_TOOLS - contains expected tools", () => {
  assertEquals(TRADING_TOOLS.includes("place_order"), true);
  assertEquals(TRADING_TOOLS.includes("cancel_order"), true);
});
