// ssmd-agent/src/cli.ts
import { checkApiVersion, validateConfig } from "./config.ts";
import { createAgent } from "./agent/graph.ts";
import { AuditLogger } from "./audit/logger.ts";

interface TokenUsage {
  input: number;
  output: number;
}

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

  // Initialize audit logger
  const audit = new AuditLogger();
  await audit.init();
  console.log(`[audit] Logging to ${audit.getLogFile()}`);

  console.log("Type 'quit' to exit\n");

  const agent = await createAgent();
  const encoder = new TextEncoder();

  while (true) {
    const input = prompt("ssmd-agent>");
    if (!input || input === "quit" || input === "exit") {
      console.log("Goodbye!");
      await audit.close();
      break;
    }

    await audit.logUserInput(input);

    try {
      const usage: TokenUsage = { input: 0, output: 0 };
      let currentResponse = "";

      for await (const event of agent.streamEvents(
        { messages: [{ role: "user", content: input }] },
        { version: "v2" }
      )) {
        switch (event.event) {
          case "on_chat_model_stream": {
            const chunk = event.data?.chunk;
            if (chunk?.content) {
              // Handle both string content and array of content blocks
              if (typeof chunk.content === "string") {
                Deno.stdout.writeSync(encoder.encode(chunk.content));
                currentResponse += chunk.content;
              } else if (Array.isArray(chunk.content)) {
                for (const block of chunk.content) {
                  if (typeof block === "string") {
                    Deno.stdout.writeSync(encoder.encode(block));
                    currentResponse += block;
                  } else if (block?.text) {
                    Deno.stdout.writeSync(encoder.encode(block.text));
                    currentResponse += block.text;
                  } else if (block?.type === "text" && block?.text) {
                    Deno.stdout.writeSync(encoder.encode(block.text));
                    currentResponse += block.text;
                  }
                }
              }
            }
            // Track usage from chunk if available
            if (chunk?.usage_metadata) {
              usage.input += chunk.usage_metadata.input_tokens ?? 0;
              usage.output += chunk.usage_metadata.output_tokens ?? 0;
            }
            break;
          }
          case "on_chat_model_end": {
            // Get final usage from the completed response
            const output = event.data?.output;
            if (output?.usage_metadata) {
              usage.input = output.usage_metadata.input_tokens ?? usage.input;
              usage.output = output.usage_metadata.output_tokens ?? usage.output;
            }
            // Log the complete response
            if (currentResponse) {
              await audit.logAssistantChunk(currentResponse);
              currentResponse = "";
            }
            break;
          }
          case "on_tool_start": {
            console.log(`\n[tool] ${event.name}(${formatArgs(event.data?.input)})`);
            await audit.logToolCall(event.name, event.data?.input);
            break;
          }
          case "on_tool_end": {
            console.log(`  → ${formatResult(event.data?.output)}`);
            await audit.logToolResult(event.name, event.data?.output);
            break;
          }
        }
      }

      // Show token usage
      if (usage.input > 0 || usage.output > 0) {
        console.log(`\n[tokens] in: ${usage.input.toLocaleString()}, out: ${usage.output.toLocaleString()}`);
        await audit.logTurnComplete(usage);
      }
      console.log("");
    } catch (e) {
      console.error(`\nError: ${(e as Error).message}\n`);
      await audit.log("error", { message: (e as Error).message });
    }
  }
}

main();
