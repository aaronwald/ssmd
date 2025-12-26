// ssmd-agent/src/agent/graph.ts
import { ChatAnthropic } from "@langchain/anthropic";
import { createReactAgent } from "@langchain/langgraph/prebuilt";
import { config } from "../config.ts";
import { allTools } from "./tools.ts";
import { loadSkills } from "./skills.ts";
import { buildSystemPrompt } from "./prompt.ts";

export async function createAgent() {
  const model = new ChatAnthropic({
    model: config.model,
    anthropicApiKey: config.anthropicApiKey,
  });

  const skills = await loadSkills();
  const systemPrompt = await buildSystemPrompt(skills);

  const agent = createReactAgent({
    llm: model,
    tools: allTools,
    messageModifier: systemPrompt,
  });

  return agent;
}
