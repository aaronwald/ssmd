# ssmd Agent Pipeline Design

## Overview

Extension to ssmd that feeds NATS market data streams into a LangGraph-based agent pipeline. Agents can define signals via natural language, and the system evaluates those signals against streaming data in real-time.

## Goals

- Stream market data (trades, orderbook) from NATS into agent pipeline
- Define signals conversationally ("alert when spread > 5%")
- Generated signal code runs without LLM in hot path (fast, deterministic)
- Persist signal events for audit and replay
- Support derived state (order book, price history) not just raw messages

## Non-Goals

- Ultra-low-latency trading execution
- Replacing ssmd's Rust connector (this extends, not replaces)
- Python runtime (using Deno/TypeScript for unified language)

## Architecture

NATS JetStream is the backbone - all components communicate via NATS, enabling independent scaling, restarts, and full observability.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              NATS JetStream                                 │
│                                                                             │
│  Raw Market Data (from ssmd connector):                                     │
│    kalshi.trades.>                                                          │
│    kalshi.orderbook.>                                                       │
│    kalshi.ticker.>                                                          │
│                                                                             │
│  Derived State (from State Builders):                                       │
│    {env}.state.orderbook.{ticker}                                           │
│    {env}.state.price-history.{ticker}                                       │
│    {env}.state.volume-profile.{ticker}                                      │
│                                                                             │
│  Signal Events (from Signal Runtime):                                       │
│    {env}.signals.fired.{signalId}                                           │
│                                                                             │
│  Actions (from Action Agent):                                               │
│    {env}.signals.actions.{signalId}                                         │
│                                                                             │
└───────┬─────────────────┬─────────────────┬─────────────────┬───────────────┘
        │                 │                 │                 │
        ▼                 ▼                 ▼                 ▼
┌───────────────┐ ┌───────────────┐ ┌───────────────┐ ┌───────────────────────┐
│ State Builder │ │ Signal Runtime│ │ Action Agent  │ │   Archiver (existing) │
│    (Deno)     │ │    (Deno)     │ │    (Deno)     │ │                       │
│               │ │               │ │               │ │                       │
│ subscribes:   │ │ subscribes:   │ │ subscribes:   │ │ subscribes:           │
│  kalshi.*     │ │  {env}.state.*│ │  {env}.signals│ │  kalshi.*             │
│               │ │               │ │    .fired.*   │ │  {env}.state.*        │
│ publishes:    │ │ publishes:    │ │               │ │  {env}.signals.*      │
│  {env}.state.*│ │  {env}.signals│ │ publishes:    │ │                       │
│               │ │    .fired.*   │ │  {env}.signals│ │ writes to: S3         │
│               │ │               │ │    .actions.* │ │                       │
└───────────────┘ └───────────────┘ └───────────────┘ └───────────────────────┘

┌───────────────────────────────────────────────────────────────────────────────┐
│                        Definition Agent (Deno + LangGraph.js)                 │
│                                                                               │
│  Separate process - not in hot path                                           │
│  User: "Alert when spread > 5%"                                               │
│          │                                                                    │
│          ▼                                                                    │
│  Understand → Generate TS → Validate → Deploy (git commit + signal reload)    │
│                                                                               │
│  signals/                              state/                                 │
│    spread-alert.ts                       orderbook.ts                         │
│    depth-imbalance.ts                    price-history.ts                     │
│    (version controlled)                  (version controlled)                 │
└───────────────────────────────────────────────────────────────────────────────┘
```

### Benefits of NATS-Centric Design

| Benefit | Description |
|---------|-------------|
| **Independent scaling** | Run multiple State Builders or Signal Runtimes |
| **Fault isolation** | Components restart without losing messages (JetStream persistence) |
| **Observability** | All communication visible, auditable via NATS |
| **Consistent pattern** | Same architecture as ssmd connector/archiver |
| **Replay** | Can replay any stream for debugging or backtesting |

## Components

### State Builders

Maintain derived state from raw NATS messages:

```typescript
// state/orderbook.ts
import type { StateBuilder, OrderbookUpdate } from "ssmd-agent/types";

export const orderbookBuilder: StateBuilder<OrderBook> = {
  id: "orderbook",
  subjects: ["kalshi.orderbook.>"],

  initialState: (): OrderBook => ({
    bids: new Map(),  // price -> size
    asks: new Map(),
    lastUpdate: 0,
  }),

  update(msg: OrderbookUpdate, state: OrderBook): void {
    if (msg.side === "bid") {
      if (msg.size === 0) state.bids.delete(msg.price);
      else state.bids.set(msg.price, msg.size);
    } else {
      if (msg.size === 0) state.asks.delete(msg.price);
      else state.asks.set(msg.price, msg.size);
    }
    state.lastUpdate = msg.timestamp;
  },

  derived: {
    bestBid: (s) => Math.max(...s.bids.keys()),
    bestAsk: (s) => Math.min(...s.asks.keys()),
    spread: (s) => (s.bestAsk - s.bestBid) / s.bestBid,
    bidDepth: (s) => [...s.bids.values()].reduce((a, b) => a + b, 0),
  },
};
```

### Signal Interface

Generated signals declare dependencies and evaluate against derived state:

```typescript
// signals/spread-alert.ts
import type { Signal } from "ssmd-agent/types";

export const signal: Signal = {
  id: "spread-alert",
  name: "Wide Spread Alert",

  requires: ["orderbook"],

  evaluate(state: { orderbook: OrderBook }): boolean {
    return state.orderbook.spread > 0.05;
  },

  payload(state) {
    return {
      spread: state.orderbook.spread,
      bestBid: state.orderbook.bestBid,
      bestAsk: state.orderbook.bestAsk,
    };
  },
};
```

### Signal Event Persistence

When a signal fires, a SignalEvent is published to NATS and archived:

```typescript
interface SignalEvent {
  id: string;              // unique event id
  signalId: string;        // "spread-alert"
  signalVersion: string;   // git sha of signal code

  firedAt: number;         // when signal evaluated true
  messageTimestamp: number; // timestamp of triggering message

  state: object;           // full state at fire time
  payload: object;         // signal's custom payload

  subjects: string[];      // NATS subjects that contributed
  messageIds: string[];    // last N message IDs that built state
}
```

**NATS subject:** `{env}.signals.{signalId}`

**S3 archive:**
```
s3://ssmd-data/signals/2025/12/23/spread-alert/events.jsonl.gz
```

### Skills (Tools)

Agents use tools to query ssmd and external systems. Tools are bound to the LLM and can be called during graph execution.

```typescript
import { tool } from "@langchain/core/tools";
import { z } from "zod";
import { NatsClient } from "./nats.ts";

// Query current orderbook state from NATS
const getOrderbook = tool(
  async ({ ticker }: { ticker: string }) => {
    const state = await nats.getLastMessage(`state.orderbook.${ticker}`);
    return {
      ticker,
      bestBid: state.bestBid,
      bestAsk: state.bestAsk,
      spread: state.spread,
      bidDepth: state.bidDepth,
      askDepth: state.askDepth,
      timestamp: state.timestamp,
    };
  },
  {
    name: "get_orderbook",
    description: "Get current orderbook state for a ticker",
    schema: z.object({ ticker: z.string() }),
  }
);

// List available state builders
const listStateBuilders = tool(
  async () => {
    const builders = await loadStateBuilders();
    return builders.map(b => ({
      id: b.id,
      description: b.description,
      subjects: b.subjects,
      derivedFields: Object.keys(b.derived),
    }));
  },
  {
    name: "list_state_builders",
    description: "List available state builders and their derived fields",
    schema: z.object({}),
  }
);

// List existing signals
const listSignals = tool(
  async () => {
    const signals = await loadSignals();
    return signals.map(s => ({
      id: s.id,
      name: s.name,
      requires: s.requires,
      description: s.description,
    }));
  },
  {
    name: "list_signals",
    description: "List existing signal definitions for reference",
    schema: z.object({}),
  }
);

// Query recent trades
const getRecentTrades = tool(
  async ({ ticker, limit }: { ticker: string; limit: number }) => {
    const trades = await nats.getHistory(`kalshi.trades.${ticker}`, limit);
    return trades.map(t => ({
      price: t.price,
      size: t.size,
      side: t.side,
      timestamp: t.timestamp,
    }));
  },
  {
    name: "get_recent_trades",
    description: "Get recent trades for a ticker",
    schema: z.object({
      ticker: z.string(),
      limit: z.number().default(100),
    }),
  }
);

// Query signal history
const getSignalHistory = tool(
  async ({ signalId, hours }: { signalId: string; hours: number }) => {
    const events = await nats.getHistory(`signals.fired.${signalId}`, {
      since: Date.now() - hours * 3600 * 1000
    });
    return events.map(e => ({
      id: e.id,
      firedAt: e.firedAt,
      ticker: e.ticker,
      payload: e.payload,
    }));
  },
  {
    name: "get_signal_history",
    description: "Get recent fire events for a signal",
    schema: z.object({
      signalId: z.string(),
      hours: z.number().default(24),
    }),
  }
);

// All available tools
const definitionTools = [listStateBuilders, listSignals, getOrderbook, getRecentTrades];
const actionTools = [getOrderbook, getRecentTrades, getSignalHistory];
```

### Definition Agent

LangGraph.js graph for creating signals from natural language:

```typescript
import { StateGraph, END } from "@langchain/langgraph";
import { ChatAnthropic } from "@langchain/anthropic";

// Bind tools to LLM
const llm = new ChatAnthropic({ model: "claude-sonnet-4-20250514" });
const llmWithTools = llm.bindTools(definitionTools);

// State flowing through the graph
interface DefinitionState {
  messages: BaseMessage[];
  userRequest: string;
  signalSpec: {
    id: string;
    name: string;
    requires: string[];      // state builders needed
    condition: string;       // natural language condition
  } | null;
  generatedCode: string | null;
  validationErrors: string[];
  approved: boolean;
}

// Nodes
async function understand(state: DefinitionState): Promise<Partial<DefinitionState>> {
  // LLM can call tools to explore available state builders and existing signals
  const response = await llmWithTools.invoke([
    { role: "system", content: UNDERSTAND_PROMPT },
    { role: "user", content: state.userRequest },
  ]);

  // Handle tool calls if LLM wants to explore
  if (response.tool_calls?.length) {
    const toolResults = await executeTools(response.tool_calls);
    const followUp = await llmWithTools.invoke([
      ...state.messages,
      response,
      ...toolResults,
    ]);
    return {
      signalSpec: parseSignalSpec(followUp.content),
      messages: [...state.messages, response, ...toolResults, followUp],
    };
  }

  return { signalSpec: parseSignalSpec(response.content) };
}

async function generate(state: DefinitionState): Promise<Partial<DefinitionState>> {
  // LLM can call tools to see example data, check current orderbook, etc.
  const response = await llmWithTools.invoke([
    { role: "system", content: GENERATE_PROMPT },
    { role: "user", content: JSON.stringify(state.signalSpec) },
  ]);
  return { generatedCode: response.content };
}

async function validate(state: DefinitionState): Promise<Partial<DefinitionState>> {
  // Type-check with Deno
  const result = await Deno.command("deno", {
    args: ["check", "--quiet", "-"],
    stdin: "piped",
  }).output();
  // ... write code to stdin, collect errors
  return { validationErrors: parseErrors(result.stderr) };
}

async function deploy(state: DefinitionState): Promise<Partial<DefinitionState>> {
  const path = `signals/${state.signalSpec!.id}.ts`;
  await Deno.writeTextFile(path, state.generatedCode!);
  await gitCommit(path, `signal: add ${state.signalSpec!.id}`);
  await notifyReload();  // publish to NATS for signal runtime to reload
  return {};
}

// Routing
function shouldRetry(state: DefinitionState): string {
  return state.validationErrors.length > 0 ? "generate" : "confirm";
}

function shouldDeploy(state: DefinitionState): string {
  return state.approved ? "deploy" : END;
}

// Graph
const definitionGraph = new StateGraph<DefinitionState>()
  .addNode("understand", understand)
  .addNode("generate", generate)
  .addNode("validate", validate)
  .addNode("confirm", confirmWithUser)  // human-in-the-loop
  .addNode("deploy", deploy)
  .addEdge("understand", "generate")
  .addEdge("generate", "validate")
  .addConditionalEdges("validate", shouldRetry, ["generate", "confirm"])
  .addConditionalEdges("confirm", shouldDeploy, ["deploy", END])
  .addEdge("deploy", END)
  .compile();

// Usage
const result = await definitionGraph.invoke({
  userRequest: "Alert me when the spread exceeds 5% on any INXD market",
  messages: [],
  signalSpec: null,
  generatedCode: null,
  validationErrors: [],
  approved: false,
});
```

Generated code is committed to git for version control and auditability.

### Action Agent

LangGraph.js graph invoked when signals fire:

```typescript
import { StateGraph, END } from "@langchain/langgraph";
import { ChatAnthropic } from "@langchain/anthropic";

// Bind tools to LLM for action agent
const actionLlm = new ChatAnthropic({ model: "claude-sonnet-4-20250514" });
const actionLlmWithTools = actionLlm.bindTools(actionTools);

interface ActionState {
  signalEvent: SignalEvent;
  context: {
    recentEvents: SignalEvent[];   // last N events for this signal
    marketContext: string;          // current market conditions
  };
  interpretation: string | null;
  action: {
    type: "alert" | "log" | "webhook" | "trade_signal";
    data: Record<string, unknown>;
  } | null;
  executed: boolean;
}

async function interpret(state: ActionState): Promise<Partial<ActionState>> {
  // LLM can call tools to get current orderbook, recent trades, signal history
  const response = await actionLlmWithTools.invoke([
    { role: "system", content: INTERPRET_PROMPT },
    { role: "user", content: JSON.stringify({
      event: state.signalEvent,
      recentHistory: state.context.recentEvents,
      market: state.context.marketContext,
    })},
  ]);

  // Handle tool calls to gather more context
  if (response.tool_calls?.length) {
    const toolResults = await executeTools(response.tool_calls);
    const followUp = await actionLlmWithTools.invoke([
      response,
      ...toolResults,
      { role: "user", content: "Now provide your interpretation based on this data." },
    ]);
    return { interpretation: followUp.content };
  }

  return { interpretation: response.content };
}

async function decide(state: ActionState): Promise<Partial<ActionState>> {
  const response = await actionLlm.invoke([
    { role: "system", content: DECIDE_PROMPT },
    { role: "user", content: state.interpretation },
  ]);
  return { action: parseAction(response.content) };
}

async function execute(state: ActionState): Promise<Partial<ActionState>> {
  const { action } = state;
  switch (action!.type) {
    case "alert":
      await sendAlert(action!.data);
      break;
    case "webhook":
      await fetch(action!.data.url, { method: "POST", body: JSON.stringify(action!.data) });
      break;
    case "trade_signal":
      await publishToNats("signals.trade", action!.data);
      break;
    case "log":
    default:
      console.log("Signal logged:", state.signalEvent.id);
  }
  // Publish action to NATS for archival
  await publishToNats(`signals.actions.${state.signalEvent.signalId}`, {
    eventId: state.signalEvent.id,
    action: action,
    interpretation: state.interpretation,
  });
  return { executed: true };
}

const actionGraph = new StateGraph<ActionState>()
  .addNode("interpret", interpret)
  .addNode("decide", decide)
  .addNode("execute", execute)
  .addEdge("interpret", "decide")
  .addEdge("decide", "execute")
  .addEdge("execute", END)
  .compile();

// Called when signal fires (from NATS subscription)
async function onSignalFired(event: SignalEvent) {
  const context = await buildContext(event);
  await actionGraph.invoke({
    signalEvent: event,
    context,
    interpretation: null,
    action: null,
    executed: false,
  });
}
```

## Technology Choices

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Runtime | Deno | Native TypeScript, sandboxed execution for generated code |
| Agent framework | LangGraph.js | Official TypeScript port, same patterns as Python |
| NATS client | nats.deno | Official Deno support |
| Signal code | TypeScript | Same language for definition and execution |
| Wire format | Cap'n Proto | Zero-copy, schema evolution, shared schemas with Rust |
| TS Cap'n Proto | capnp-es | Modern TypeScript v5 implementation |

## Wire Format

All NATS messages use Cap'n Proto. Same `.capnp` schema files generate both Rust and TypeScript types.

```capnp
# schemas/state.capnp
struct OrderBookState {
  ticker @0 :Text;
  timestamp @1 :UInt64;
  bids @2 :List(Level);
  asks @3 :List(Level);
  bestBid @4 :Float64;
  bestAsk @5 :Float64;
  spread @6 :Float64;
}

struct Level {
  price @0 :Float64;
  size @1 :Float64;
}

# schemas/signal.capnp
struct SignalEvent {
  id @0 :Text;
  signalId @1 :Text;
  signalVersion @2 :Text;
  firedAt @3 :UInt64;
  ticker @4 :Text;
  state @5 :Data;     # serialized state snapshot
  payload @6 :Data;   # signal-specific payload
}

struct SignalAction {
  eventId @0 :Text;
  signalId @1 :Text;
  actionType @2 :Text;  # alert, log, webhook, trade
  actionData @3 :Data;  # action-specific payload
  decidedAt @4 :UInt64;
}
```

**Build pipeline:**
```
schemas/*.capnp
    │
    ├──▶ capnp compile --rust → ssmd-rust/crates/schema/src/
    │
    └──▶ capnp-es generate  → ssmd-agent/src/schema/
```

## Data Flow

All communication via NATS - each arrow is a NATS publish/subscribe:

```
Kalshi WS → Connector → NATS: kalshi.trades.*, kalshi.orderbook.*
                                    │
                                    ▼
                        State Builder (Deno)
                                    │
                                    ▼
                        NATS: {env}.state.orderbook.*, {env}.state.price-history.*
                                    │
                                    ▼
                        Signal Runtime (Deno)
                                    │
                                    ▼
                        NATS: {env}.signals.fired.*
                                    │
                          ┌─────────┴─────────┐
                          ▼                   ▼
                   Action Agent         Archiver
                      (Deno)            (existing)
                          │                   │
                          ▼                   ▼
           NATS: {env}.signals.actions.*     S3
```

**Signal creation flow (separate, not in hot path):**

```
User chat → Definition Agent → generates .ts → git commit → signal reload via NATS
```

## Open Questions

1. **Hot reload** - How does runtime detect new/changed signals? File watcher or explicit reload command?
2. **Multi-ticker state** - Does each ticker get its own OrderBook instance, or shared state?
3. **Backpressure** - What happens if signal evaluation can't keep up with message rate?
4. **Testing** - How to test generated signals before deploying to production?

## Future Considerations

- Polymarket, Kraken connectors feed same pipeline
- Web UI for signal management (view, enable/disable, history)
- Backtesting: replay historical data through signals
- Signal marketplace: share/import signal definitions

---

*Design created: 2025-12-23*
*Updated: 2025-12-23 - NATS-centric architecture (all components communicate via NATS)*
