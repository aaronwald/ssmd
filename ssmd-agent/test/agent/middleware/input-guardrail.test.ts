import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  applyInputGuardrail,
  trimMessages,
  type Message,
} from "../../../src/agent/middleware/input-guardrail.ts";

const createMessages = (count: number): Message[] =>
  Array.from({ length: count }, (_, i) => ({
    role: i % 2 === 0 ? "user" : "assistant",
    content: `Message ${i + 1}`,
  }));

Deno.test("trimMessages - keeps messages under limit", () => {
  const messages = createMessages(5);
  const result = trimMessages(messages, 10);
  assertEquals(result.length, 5);
});

Deno.test("trimMessages - trims oldest messages first", () => {
  const messages = createMessages(10);
  const result = trimMessages(messages, 5);
  assertEquals(result.length, 5);
  assertEquals(result[0].content, "Message 6");
  assertEquals(result[4].content, "Message 10");
});

Deno.test("trimMessages - always keeps system message", () => {
  const messages: Message[] = [
    { role: "system", content: "You are a helpful assistant" },
    ...createMessages(10),
  ];
  const result = trimMessages(messages, 5);
  assertEquals(result.length, 5);
  assertEquals(result[0].role, "system");
});

Deno.test("applyInputGuardrail - passes through under limit", () => {
  const messages = createMessages(5);
  const result = applyInputGuardrail(messages, { maxMessages: 10 });
  assertEquals(result.messages.length, 5);
  assertEquals(result.trimmed, false);
});

Deno.test("applyInputGuardrail - trims when over limit", () => {
  const messages = createMessages(15);
  const result = applyInputGuardrail(messages, { maxMessages: 10 });
  assertEquals(result.messages.length, 10);
  assertEquals(result.trimmed, true);
});

Deno.test("applyInputGuardrail - uses default max if not specified", () => {
  const messages = createMessages(100);
  const result = applyInputGuardrail(messages);
  assertEquals(result.messages.length <= 50, true); // Default is 50
});
