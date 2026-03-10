import type { StageConfig, StageResult } from "../types.ts";
import { MAX_OUTPUT_SIZE } from "../types.ts";
import type { ExecuteContext } from "./mod.ts";

export async function executeOpenRouter(
  config: StageConfig,
  ctx: ExecuteContext,
  signal: AbortSignal,
): Promise<StageResult> {
  const model = config.model;
  const systemPrompt = config.system_prompt ?? "You are an automated pipeline analysis agent. Analyze the provided data and respond concisely.";
  const userPrompt = config.user_prompt ?? config.prompt;

  if (!model) {
    return { status: "failed", error: "openrouter stage requires 'model' in config" };
  }
  if (!userPrompt) {
    return { status: "failed", error: "openrouter stage requires 'user_prompt' (or 'prompt') in config" };
  }

  const maxTokens = config.max_tokens ?? 2000;
  const temperature = config.temperature ?? 0.3;

  try {
    const resp = await fetch(`${ctx.dataTsUrl}/v1/chat/completions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": `Bearer ${ctx.adminApiKey}`,
      },
      body: JSON.stringify({
        model,
        messages: [
          { role: "system", content: systemPrompt },
          { role: "user", content: userPrompt },
        ],
        max_tokens: maxTokens,
        temperature,
        stream: false,
      }),
      signal,
    });

    if (!resp.ok) {
      const text = await resp.text();
      return { status: "failed", error: `OpenRouter proxy returned ${resp.status}: ${text.slice(0, 500)}` };
    }

    const data = await resp.json();
    const content = data?.choices?.[0]?.message?.content ?? "";

    const truncated = content.length > MAX_OUTPUT_SIZE;
    const output = truncated ? content.slice(0, MAX_OUTPUT_SIZE) : content;

    return {
      status: "completed",
      output: { content: output, model, truncated, usage: data?.usage },
    };
  } catch (err) {
    return {
      status: "failed",
      error: err instanceof Error ? err.message : String(err),
    };
  }
}
