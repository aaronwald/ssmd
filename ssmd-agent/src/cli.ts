// ssmd-agent/src/cli.ts
import { parseArgs } from "jsr:@std/cli/parse-args";
import { checkApiVersion, validateConfig } from "./config.ts";
import { createAgent } from "./agent/graph.ts";
import { EventLogger } from "./audit/events.ts";
import {
  applyOutputGuardrail,
  applyToolGuardrail,
  type ToolCall,
} from "./agent/middleware/mod.ts";
import { formatArgs, formatResult } from "./cli-utils.ts";

// Extended message interface for LangChain messages with optional runtime properties
// BaseMessage doesn't include these, but AIMessage has them at runtime
interface AgentMessage {
  content: string | unknown;
  _getType?: () => string;
  usage_metadata?: {
    input_tokens?: number;
    output_tokens?: number;
  };
  tool_calls?: Array<{
    name: string;
    args?: Record<string, unknown>;
  }>;
}

const args = parseArgs(Deno.args, {
  string: ["prompt", "p"],
  alias: { p: "prompt" },
});

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

    // Cast messages to AgentMessage for type-safe access to runtime properties
    const messages = result.messages as AgentMessage[];

    for (const msg of messages) {
      // Track token usage
      if (msg.usage_metadata) {
        totalInputTokens += msg.usage_metadata.input_tokens ?? 0;
        totalOutputTokens += msg.usage_metadata.output_tokens ?? 0;
      }

      // Show tool calls with guardrail check
      if (msg.tool_calls && msg.tool_calls.length > 0) {
        // Convert to ToolCall format for guardrail
        const toolCalls: ToolCall[] = msg.tool_calls.map((tc) => ({
          name: tc.name,
          args: tc.args ?? {},
        }));

        const toolGuardResult = applyToolGuardrail(toolCalls);

        // Show approved tool calls
        for (const tc of toolGuardResult.approvedCalls ?? []) {
          console.log(`[tool] ${tc.name}(${formatArgs(tc.args)})`);
        }

        // Warn about trading tools requiring approval
        for (const tc of toolGuardResult.pendingApproval ?? []) {
          console.log(`[guardrail] ⚠️  Trading tool blocked: ${tc.name}(${formatArgs(tc.args)})`);
          console.log(`[guardrail] Trading operations require human approval`);
        }

        // Show rejected tools
        for (const tc of toolGuardResult.rejectedCalls ?? []) {
          console.log(`[guardrail] ❌ Unknown tool rejected: ${tc.name} - ${tc.reason}`);
        }
      }

      // Show tool results
      if (msg._getType?.() === "tool") {
        console.log(`  → ${formatResult(msg.content)}`);
      }
    }

    // Show final AI response with output guardrail
    const lastMsg = messages[messages.length - 1];
    if (lastMsg.content) {
      const outputGuardResult = applyOutputGuardrail(String(lastMsg.content));
      if (outputGuardResult.allowed) {
        console.log(lastMsg.content);
      } else {
        console.log(`[guardrail] ❌ Response blocked: ${outputGuardResult.reason}`);
        await logger.logEvent({ event: "guardrail_block", data: { reason: outputGuardResult.reason } });
      }
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
