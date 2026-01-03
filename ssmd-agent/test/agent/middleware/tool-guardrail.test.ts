import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  applyToolGuardrail,
  type ToolGuardrailResult,
} from "../../../src/agent/middleware/tool-guardrail.ts";

Deno.test("applyToolGuardrail - allows safe tools", () => {
  const toolCalls = [{ name: "get_markets", args: {} }];
  const result = applyToolGuardrail(toolCalls);
  assertEquals(result.allowed, true);
  assertEquals(result.approvedCalls?.length, 1);
});

Deno.test("applyToolGuardrail - blocks trading tools requiring approval", () => {
  const toolCalls = [{ name: "place_order", args: { ticker: "TEST" } }];
  const result = applyToolGuardrail(toolCalls);
  assertEquals(result.allowed, false);
  assertEquals(result.pendingApproval?.length, 1);
});

Deno.test("applyToolGuardrail - handles mixed safe and trading tools", () => {
  const toolCalls = [
    { name: "get_markets", args: {} },
    { name: "place_order", args: { ticker: "TEST" } },
  ];
  const result = applyToolGuardrail(toolCalls);
  assertEquals(result.allowed, false);
  assertEquals(result.approvedCalls?.length, 1);
  assertEquals(result.pendingApproval?.length, 1);
});

Deno.test("applyToolGuardrail - blocks unknown tools", () => {
  const toolCalls = [{ name: "dangerous_unknown", args: {} }];
  const result = applyToolGuardrail(toolCalls);
  assertEquals(result.allowed, false);
  assertEquals(result.rejectedCalls?.length, 1);
});

Deno.test("applyToolGuardrail - allows trading when approval disabled", () => {
  const toolCalls = [{ name: "place_order", args: { ticker: "TEST" } }];
  const result = applyToolGuardrail(toolCalls, { tradingApprovalEnabled: false });
  assertEquals(result.allowed, true);
});

Deno.test("applyToolGuardrail - returns empty arrays for no tool calls", () => {
  const result = applyToolGuardrail([]);
  assertEquals(result.allowed, true);
  assertEquals(result.approvedCalls?.length, 0);
});
