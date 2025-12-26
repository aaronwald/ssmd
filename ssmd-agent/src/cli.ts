// ssmd-agent/src/cli.ts
import { validateConfig } from "./config.ts";
import { createAgent } from "./agent/graph.ts";

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
  console.log("Type 'quit' to exit\n");

  const agent = await createAgent();
  const encoder = new TextEncoder();

  while (true) {
    const input = prompt("ssmd-agent>");
    if (!input || input === "quit" || input === "exit") {
      console.log("Goodbye!");
      break;
    }

    try {
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
              } else if (Array.isArray(chunk.content)) {
                for (const block of chunk.content) {
                  if (typeof block === "string") {
                    Deno.stdout.writeSync(encoder.encode(block));
                  } else if (block?.text) {
                    Deno.stdout.writeSync(encoder.encode(block.text));
                  } else if (block?.type === "text" && block?.text) {
                    Deno.stdout.writeSync(encoder.encode(block.text));
                  }
                }
              }
            }
            break;
          }
          case "on_tool_start": {
            console.log(`\n[tool] ${event.name}(${formatArgs(event.data?.input)})`);
            break;
          }
          case "on_tool_end": {
            console.log(`  → ${formatResult(event.data?.output)}`);
            break;
          }
        }
      }
      console.log("\n");
    } catch (e) {
      console.error(`\nError: ${(e as Error).message}\n`);
    }
  }
}

main();
