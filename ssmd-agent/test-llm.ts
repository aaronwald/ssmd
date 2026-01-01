// Simple test to debug ChatOpenAI with our proxy
import { ChatOpenAI } from "@langchain/openai";
import { tool } from "@langchain/core/tools";
import { z } from "zod";

const apiUrl = Deno.env.get("SSMD_API_URL") ?? "http://localhost:8080";
const apiKey = Deno.env.get("SSMD_DATA_API_KEY") ?? "";
const modelName = Deno.env.get("SSMD_MODEL") ?? "anthropic/claude-sonnet-4";

console.log("Testing ChatOpenAI with proxy");
console.log("API URL:", apiUrl);
console.log("Model:", modelName);
console.log("");

const chat = new ChatOpenAI({
  model: modelName,
  apiKey,
  streaming: false,
  configuration: {
    baseURL: `${apiUrl}/v1`,
  },
});

// Test 1: Simple invoke
console.log("=== Test 1: Simple invoke ===");
try {
  const response = await chat.invoke("Say hello in one word");
  console.log("Content:", response.content);
} catch (e) {
  console.error("Error:", e);
}

// Test 2: With tools bound
console.log("\n=== Test 2: With tools ===");
const getDate = tool(
  async () => new Date().toISOString().split("T")[0],
  {
    name: "get_date",
    description: "Get today's date",
    schema: z.object({}),
  }
);

const chatWithTools = chat.bindTools([getDate]);

try {
  const response = await chatWithTools.invoke("What is today's date?");
  console.log("Content:", response.content);
  console.log("Tool calls:", response.tool_calls);
} catch (e) {
  console.error("Error:", e);
}
