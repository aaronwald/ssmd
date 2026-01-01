// ssmd-agent/src/cli.ts
import { parseArgs } from "jsr:@std/cli/parse-args";
import { checkApiVersion, validateConfig } from "./config.ts";
import { createAgent } from "./agent/graph.ts";
import { EventLogger } from "./audit/events.ts";

interface TokenUsage {
  input: number;
  output: number;
}

const args = parseArgs(Deno.args, {
  string: ["prompt", "p"],
  alias: { p: "prompt" },
});

function formatArgs(input: unknown): string {
  if (typeof input === "object" && input !== null) {
    const obj = input as Record<string, unknown>;
    const parts = Object.entries(obj)
      .filter(([_, v]) => v !== undefined)
      .map(([k, v]) => `${k}=${JSON.stringify(v)}`);
    return parts.join(", ");
  }
  return String(input);
}

function formatResult(output: unknown): string {
  if (typeof output === "string") {
    try {
      const parsed = JSON.parse(output);
      if (Array.isArray(parsed)) {
        return `${parsed.length} items`;
      }
      if (parsed.count !== undefined) {
        return `${parsed.count} snapshots`;
      }
      if (parsed.fires !== undefined) {
        return `${parsed.fires} fires, ${parsed.errors?.length ?? 0} errors`;
      }
      if (parsed.sha) {
        return `Committed: ${parsed.sha}`;
      }
      return output.slice(0, 100) + (output.length > 100 ? "..." : "");
    } catch {
      return output.slice(0, 100) + (output.length > 100 ? "..." : "");
    }
  }
  return String(output);
}

async function main() {
  try {
    validateConfig();
  } catch (e) {
    console.error((e as Error).message);
    Deno.exit(1);
  }

  console.log("ssmd-agent v0.1.0");

  // Check API version compatibility (non-blocking warning)
  await checkApiVersion();

  // Initialize event logger
  const logger = new EventLogger();
  await logger.init();
  console.log(`[audit] ${logger.getLogFile()}`);

  const agent = await createAgent();
  const encoder = new TextEncoder();

  // Single prompt mode: run once and exit
  const singlePrompt = args.prompt;
  if (singlePrompt) {
    await runPrompt(agent, singlePrompt, logger, encoder);
    await logger.close();
    return;
  }

  console.log("Type 'quit' to exit\n");

  while (true) {
    const input = prompt("ssmd-agent>");
    if (!input || input === "quit" || input === "exit") {
      console.log("Goodbye!");
      await logger.close();
      break;
    }

    await runPrompt(agent, input, logger, encoder);
  }
}

async function runPrompt(
  agent: Awaited<ReturnType<typeof createAgent>>,
  input: string,
  logger: EventLogger,
  _encoder: TextEncoder
) {
  try {
    await logger.logEvent({ event: "user_input", data: { content: input } });

    // Use invoke instead of streamEvents (streamEvents has issues with non-streaming proxy)
    const result = await agent.invoke({ messages: [{ role: "user", content: input }] });
    await logger.logEvent({ event: "agent_result", data: result });

    // Process messages to show tool calls and final response
    let totalInputTokens = 0;
    let totalOutputTokens = 0;

    for (const msg of result.messages) {
      // Track token usage
      if (msg.usage_metadata) {
        totalInputTokens += msg.usage_metadata.input_tokens ?? 0;
        totalOutputTokens += msg.usage_metadata.output_tokens ?? 0;
      }

      // Show tool calls
      if (msg.tool_calls && msg.tool_calls.length > 0) {
        for (const tc of msg.tool_calls) {
          console.log(`[tool] ${tc.name}(${formatArgs(tc.args)})`);
        }
      }

      // Show tool results
      if (msg._getType?.() === "tool") {
        console.log(`  → ${formatResult(msg.content)}`);
      }
    }

    // Show final AI response
    const lastMsg = result.messages[result.messages.length - 1];
    if (lastMsg.content) {
      console.log(lastMsg.content);
    }

    // Show token usage
    if (totalInputTokens > 0 || totalOutputTokens > 0) {
      console.log(`\n[tokens] in: ${totalInputTokens.toLocaleString()}, out: ${totalOutputTokens.toLocaleString()}`);
    }
    console.log("");
  } catch (e) {
    console.error(`\nError: ${(e as Error).message}\n`);
  }
}

main();
