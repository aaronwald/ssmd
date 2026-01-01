// Simple test to debug ChatOpenAI with our proxy
import { ChatOpenAI } from "@langchain/openai";
import { tool } from "@langchain/core/tools";
import { z } from "zod";
import { createReactAgent } from "@langchain/langgraph/prebuilt";

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

const getDate = tool(
  async () => new Date().toISOString().split("T")[0],
  {
    name: "get_date",
    description: "Get today's date",
    schema: z.object({}),
  }
);

// Test 3: Full agent with invoke
console.log("=== Test 3: Agent with invoke ===");
const agent = createReactAgent({
  llm: chat,
  tools: [getDate],
});

try {
  const response = await agent.invoke({ messages: [{ role: "user", content: "hi" }] });
  console.log("Response:", response.messages[response.messages.length - 1].content);
} catch (e) {
  console.error("Error:", e);
}

// Test 4: Agent with streamEvents
console.log("\n=== Test 4: Agent with streamEvents ===");
try {
  for await (const event of agent.streamEvents(
    { messages: [{ role: "user", content: "hi" }] },
    { version: "v2" }
  )) {
    if (event.event === "on_chat_model_end") {
      console.log("Chat model output:", event.data?.output?.content);
    }
  }
} catch (e) {
  console.error("Error:", e);
}
