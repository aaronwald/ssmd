// ssmd-agent/src/agent/graph.ts
import { ChatOpenAI } from "@langchain/openai";
import { createReactAgent } from "@langchain/langgraph/prebuilt";
import type { LanguageModelLike } from "@langchain/core/language_models/base";
import { config } from "../config.ts";
import { allTools } from "./tools.ts";
import { loadSkills } from "./skills.ts";
import { buildSystemPrompt } from "./prompt.ts";

export async function createAgent() {
  // Use ChatOpenAI with custom baseURL pointing to ssmd-data proxy
  // This routes all LLM calls through our proxy for token tracking and guardrails
  const model = new ChatOpenAI({
    model: config.model, // OpenRouter format: "anthropic/claude-sonnet-4"
    apiKey: config.apiKey,
    streaming: false, // Disable streaming - our proxy expects JSON, not SSE chunks
    temperature: 0, // Explicit to avoid any defaults
    configuration: {
      baseURL: `${config.apiUrl}/v1`,
      defaultHeaders: {
        "HTTP-Referer": "https://ssmd.varshtat.com",
      },
    },
  });

  const skills = await loadSkills();
  const systemPrompt = await buildSystemPrompt(skills);

  // Type assertion needed due to langchain package version incompatibility
  // Runtime behavior is compatible - the ChatOpenAI class implements the required interface
  const agent = createReactAgent({
    llm: model as unknown as LanguageModelLike,
    tools: allTools,
    messageModifier: systemPrompt,
  });

  return agent;
}
