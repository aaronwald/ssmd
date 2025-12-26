# LangGraph.js + NATS JetStream Agent Architecture

## Overview

Self-hosted ReAct agent using LangGraph.js as the orchestration layer with NATS JetStream as the transport. No LangGraph Server required — invoke the agent directly from your own consumer service.

## Architecture

```
NATS JetStream (requests) → Consumer Service → LangGraph Agent → NATS (responses)
                                   ↓
                              PostgreSQL (checkpoints)
```

## Core Dependencies

```bash
npm install @langchain/langgraph @langchain/langgraph-checkpoint-postgres \
            @langchain/openai @langchain/core nats pg
```

## Agent Consumer Implementation

```typescript
import { connect, JSONCodec, ConsumerMessages } from "nats";
import { createReactAgent } from "@langchain/langgraph/prebuilt";
import { ChatOpenAI } from "@langchain/openai";
import { PostgresSaver } from "@langchain/langgraph-checkpoint-postgres";

// --- Checkpointer Setup ---
const checkpointer = PostgresSaver.fromConnString(process.env.POSTGRES_URI!);
await checkpointer.setup();

// --- Agent Setup ---
const agent = createReactAgent({
  llm: new ChatOpenAI({ model: "gpt-4o" }),
  tools: [/* your tools */],
  checkpointer,
});

// --- NATS Setup ---
const nc = await connect({ servers: process.env.NATS_URL });
const js = nc.jetstream();
const codec = JSONCodec<AgentRequest>();

interface AgentRequest {
  thread_id: string;
  user_id: string;
  content: string;
  reply_subject?: string;
}

// --- Consumer Loop ---
const consumer = await js.consumers.get("AGENTS", "agent-worker");
const messages: ConsumerMessages = await consumer.consume();

for await (const msg of messages) {
  const request = codec.decode(msg.data);

  try {
    const result = await agent.invoke(
      { messages: [{ role: "user", content: request.content }] },
      { configurable: { thread_id: request.thread_id } }
    );

    const response = result.messages[result.messages.length - 1];

    if (request.reply_subject) {
      nc.publish(request.reply_subject, JSONCodec().encode({
        thread_id: request.thread_id,
        content: response.content,
      }));
    }

    msg.ack();
  } catch (err) {
    console.error(`Failed processing ${request.thread_id}:`, err);
    msg.nak();
  }
}
```

## Streaming Responses

```typescript
const streamSubject = `agent.stream.${request.thread_id}`;

const stream = await agent.stream(
  { messages: [{ role: "user", content: request.content }] },
  { configurable: { thread_id: request.thread_id }, streamMode: "messages" }
);

for await (const [message, metadata] of stream) {
  if (message.content) {
    nc.publish(streamSubject, JSONCodec().encode({
      type: "token",
      content: message.content,
      node: metadata.langgraph_node,
    }));
  }
}

nc.publish(streamSubject, JSONCodec().encode({ type: "done" }));
msg.ack();
```

## JetStream Configuration

```typescript
const jsm = await nc.jetstreamManager();

// Request stream
await jsm.streams.add({
  name: "AGENTS",
  subjects: ["agent.request.*"],
  retention: "workqueue",
  storage: "file",
});

// Durable consumer
await jsm.consumers.add("AGENTS", {
  durable_name: "agent-worker",
  ack_policy: "explicit",
  max_deliver: 3,
  ack_wait: 60_000,
  max_ack_pending: 10,  // Concurrency limit per instance
});

// Response stream
await jsm.streams.add({
  name: "AGENT_RESPONSES",
  subjects: ["agent.response.*", "agent.stream.*"],
  retention: "limits",
  max_age: 86400_000_000_000,
});
```

## Thread ID Strategies

| Strategy | Thread ID | Use Case |
|----------|-----------|----------|
| Session-based | `user_id:session_id` | Chat sessions with clear start/end |
| Continuous | `user_id` | Single ongoing conversation per user |
| Request-scoped | `uuid()` | Stateless, no memory between requests |
| Entity-scoped | `user_id:entity_id` | Conversations about specific entities |

## Scaling

- **Horizontal scaling**: Multiple consumer instances, JetStream distributes via workqueue retention
- **Backpressure**: Control via `max_ack_pending` on consumer
- **Long-running agents**: Use `msg.working()` to extend ack deadline

```typescript
const heartbeat = setInterval(() => msg.working(), 30_000);
try {
  await agent.invoke(/* ... */);
  msg.ack();
} finally {
  clearInterval(heartbeat);
}
```

## Checkpointer Options

| Option | Durability | Use Case |
|--------|-----------|----------|
| `MemorySaver` | None (in-memory) | Development, stateless agents |
| `SqliteSaver` | Single-node | Simple deployments |
| `PostgresSaver` | Full | Production, multi-instance |
| Custom (Redis) | Configurable | If you want Redis-backed state |

## Custom Graph (Alternative to createReactAgent)

```typescript
import { StateGraph, MessagesAnnotation } from "@langchain/langgraph";
import { ToolNode } from "@langchain/langgraph/prebuilt";

const model = new ChatOpenAI({ model: "gpt-4o" }).bindTools(tools);

const callModel = async (state: typeof MessagesAnnotation.State) => {
  const response = await model.invoke(state.messages);
  return { messages: [response] };
};

const shouldContinue = (state: typeof MessagesAnnotation.State) => {
  const last = state.messages[state.messages.length - 1];
  return last.tool_calls?.length ? "tools" : "__end__";
};

export const graph = new StateGraph(MessagesAnnotation)
  .addNode("agent", callModel)
  .addNode("tools", new ToolNode(tools))
  .addEdge("__start__", "agent")
  .addConditionalEdges("agent", shouldContinue)
  .addEdge("tools", "agent")
  .compile({ checkpointer });
```

## Key LangGraph Concepts

- **State**: Shared data structure (TypeScript interface) with reducers for merge behavior
- **Nodes**: Async functions that receive state, do work, return updated state
- **Edges**: Control flow — fixed transitions or conditional branches
- **Checkpointer**: Persistence layer saving state at each super-step, enables memory/replay/fault-tolerance

## References

- LangGraph.js docs: https://langchain-ai.github.io/langgraphjs/
- Checkpoint package: `@langchain/langgraph-checkpoint-postgres`
- NATS.js: https://github.com/nats-io/nats.js