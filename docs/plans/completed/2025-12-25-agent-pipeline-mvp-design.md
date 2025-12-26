# Agent Pipeline MVP Design

**Date:** 2025-12-25
**Status:** Approved

## Overview

Interactive REPL for signal development. Agents help developers generate, validate, and deploy TypeScript signal code using historical market data.

Key insight: Agents operate at **development time**, not runtime. They generate code that runs in a fast signal runtime (no LLM in hot path).

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Developer Workstation                        │
│                                                                  │
│  ┌──────────────┐         ┌──────────────┐                      │
│  │  Agent REPL  │────────▶│  ssmd-data   │◀──── JSONL.gz files  │
│  │   (Deno)     │  HTTP   │   (Go API)   │      (local/GCS)     │
│  │              │         │              │                      │
│  │ LangGraph.js │         │ /health      │                      │
│  │ Anthropic    │         │ /datasets    │                      │
│  │              │         │ /sample      │                      │
│  └──────┬───────┘         │ /schema      │                      │
│         │                 │ /builders    │                      │
│         │ writes          └──────────────┘                      │
│         ▼                                                       │
│  signals/                                                       │
│    spread-alert.ts ──▶ git commit                               │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Primary goal | Interactive REPL | Developer tool for signal iteration |
| Data access | HTTP API | Clean interface, future auth/rate-limiting |
| Data service | Separate `ssmd-data` binary (Go) | Reuses existing `internal/data/` logic |
| Auth | API key (`X-API-Key` header) | Simple for MVP, establishes pattern |
| REPL interface | Pure CLI | Fast to build, streaming output |
| State builders | OrderBook only | Covers spread alerts, proves pipeline |
| Backtest execution | Deno sandbox | Restricted permissions for safety |
| Signal deployment | Git commit only | Runtime comes later |
| LLM model | Configurable, default Sonnet | Flexibility for cost/quality tradeoff |
| Thread model | Single per session | No persistence needed for MVP |
| Tools | Explicit, builders are tools | Agent chooses which builder to use |
| Prompts/skills | Markdown files | Flexible, version controlled |
| Output | LangGraph streaming | Real-time progress visibility |

## Backlog

- JWT/OAuth authentication (post-MVP)
- PriceHistory builder
- VolumeProfile builder
- Signal runtime (live NATS subscription)
- Memory/persistence across sessions
- TUI or Web UI

## Components

### 1. ssmd-data (Go HTTP API)

**Binary:** `cmd/ssmd-data/main.go`

**Endpoints:**

| Endpoint | Method | Description |
|----------|--------|-------------|
| `GET /health` | - | Health check |
| `GET /datasets` | `?feed=&from=&to=` | List available datasets |
| `GET /datasets/{feed}/{date}/sample` | `?ticker=&type=&limit=` | Sample records |
| `GET /schema/{feed}/{type}` | - | Schema for message type |
| `GET /builders` | - | List available state builders |

**Auth:** `X-API-Key` header required. Validated against `SSMD_API_KEY` env var.

**Environment:**

| Variable | Required | Description |
|----------|----------|-------------|
| `SSMD_DATA_PATH` | Yes | Local JSONL storage path |
| `SSMD_GCS_BUCKET` | No | Optional GCS fallback |
| `SSMD_API_KEY` | Yes | Required API key |
| `PORT` | No | Default 8080 |

**Implementation:**
- Extract `internal/data/` logic into `internal/dataservice/`
- HTTP handlers in `internal/api/`
- Middleware for API key validation

### 2. ssmd-agent (Deno REPL)

**Location:** `ssmd-agent/`

**Structure:**
```
ssmd-agent/
├── Dockerfile
├── deno.json
├── skills/
│   ├── explore-data.md
│   ├── interpret-backtest.md
│   ├── monitor-spread.md
│   └── custom-signal.md
└── src/
    ├── cli.ts              # REPL entry point
    ├── agent/
    │   ├── graph.ts        # LangGraph definition
    │   ├── tools.ts        # Tool definitions
    │   ├── skills.ts       # Skill loader
    │   └── prompt.ts       # System prompt builder
    ├── state/
    │   ├── types.ts        # StateBuilder interface
    │   └── orderbook.ts    # OrderBookBuilder
    └── backtest/
        └── runner.ts       # Backtest execution
```

**Environment:**

| Variable | Required | Description |
|----------|----------|-------------|
| `SSMD_DATA_URL` | Yes | ssmd-data service URL |
| `SSMD_DATA_API_KEY` | Yes | API key for data service |
| `ANTHROPIC_API_KEY` | Yes | Claude API key |
| `SSMD_MODEL` | No | Model ID (default: claude-sonnet-4-20250514) |

### 3. Tools

All tools are explicit. Builders are tools the agent chooses to use.

| Tool | Input | Output |
|------|-------|--------|
| `list_datasets` | feed?, from?, to? | Dataset metadata array |
| `sample_data` | feed, date, ticker?, type?, limit? | Raw records array |
| `get_schema` | feed, type | Field definitions |
| `orderbook_builder` | records | OrderBookState snapshots |
| `run_backtest` | signal_code, states | BacktestResult |
| `deploy_signal` | code, path | Commit SHA |

**Tool implementations call ssmd-data API:**

```typescript
// src/agent/tools.ts
export const listDatasets = tool(
  async ({ feed, from, to }) => {
    const params = new URLSearchParams();
    if (feed) params.set("feed", feed);
    if (from) params.set("from", from);
    if (to) params.set("to", to);

    const res = await fetch(`${SSMD_DATA_URL}/datasets?${params}`, {
      headers: { "X-API-Key": SSMD_DATA_API_KEY },
    });
    return res.json();
  },
  {
    name: "list_datasets",
    description: "List available market data datasets",
    schema: z.object({
      feed: z.string().optional(),
      from: z.string().optional(),
      to: z.string().optional(),
    }),
  }
);
```

### 4. State Builders

**Interface:**

```typescript
// src/state/types.ts
export interface StateBuilder<T> {
  id: string;
  update(record: MarketRecord): void;
  getState(): T;
  reset(): void;
}
```

**OrderBookBuilder (MVP):**

```typescript
// src/state/orderbook.ts
export interface OrderBookState {
  ticker: string;
  bestBid: number;
  bestAsk: number;
  spread: number;
  spreadPercent: number;
  lastUpdate: number;
}

export class OrderBookBuilder implements StateBuilder<OrderBookState> {
  id = "orderbook";
  private state: OrderBookState;

  update(record: MarketRecord): void {
    if (record.type !== "orderbook") return;
    this.state = {
      ticker: record.ticker,
      bestBid: record.yes_bid,
      bestAsk: record.yes_ask,
      spread: record.yes_ask - record.yes_bid,
      spreadPercent: (record.yes_ask - record.yes_bid) / record.yes_ask,
      lastUpdate: record.ts,
    };
  }

  getState(): OrderBookState {
    return { ...this.state };
  }

  reset(): void {
    this.state = { ticker: "", bestBid: 0, bestAsk: 0, spread: 0, spreadPercent: 0, lastUpdate: 0 };
  }
}
```

**orderbook_builder tool:**

```typescript
export const orderbookBuilder = tool(
  async ({ records }) => {
    const builder = new OrderBookBuilder();
    const snapshots: OrderBookState[] = [];

    for (const record of records) {
      builder.update(record);
      snapshots.push(builder.getState());
    }

    return snapshots;
  },
  {
    name: "orderbook_builder",
    description: "Process market records through OrderBook state builder",
    schema: z.object({
      records: z.array(z.any()),
    }),
  }
);
```

### 5. Backtest Runner

```typescript
// src/backtest/runner.ts
interface BacktestResult {
  fires: number;
  errors: string[];
  fireTimes: string[];
  samplePayloads: object[];
  recordsProcessed: number;
  durationMs: number;
}

export const runBacktest = tool(
  async ({ signalCode, states }) => {
    const start = Date.now();
    const errors: string[] = [];
    const fires: { time: string; payload: object }[] = [];

    // Compile signal in sandbox
    const signal = await compileSignal(signalCode);

    for (const state of states) {
      try {
        if (signal.evaluate({ orderbook: state })) {
          fires.push({
            time: new Date(state.lastUpdate).toISOString(),
            payload: signal.payload({ orderbook: state }),
          });
        }
      } catch (e) {
        errors.push(e.message);
      }
    }

    return {
      fires: fires.length,
      errors,
      fireTimes: fires.slice(0, 20).map(f => f.time),
      samplePayloads: fires.slice(0, 5).map(f => f.payload),
      recordsProcessed: states.length,
      durationMs: Date.now() - start,
    };
  },
  {
    name: "run_backtest",
    description: "Evaluate signal code against state snapshots",
    schema: z.object({
      signalCode: z.string(),
      states: z.array(z.any()),
    }),
  }
);
```

**Sandbox execution:**

Signal code runs with restricted Deno permissions (`--deny-net --deny-write --deny-run`). Uses dynamic import with data URL:

```typescript
async function compileSignal(code: string) {
  const dataUrl = `data:text/typescript;base64,${btoa(code)}`;
  const module = await import(dataUrl);
  return module.signal;
}
```

### 6. Skills System

**Location:** `ssmd-agent/skills/`

**Format:**
```markdown
---
name: monitor-spread
description: Generate spread monitoring signals
---

# Spread Monitoring

Use the orderbook_builder tool to process records, then run_backtest to validate.

## Workflow

1. list_datasets to find available data
2. sample_data to get orderbook records
3. orderbook_builder to get state snapshots
4. Generate signal code using template below
5. run_backtest to validate
6. deploy_signal if results look good

## Template

\`\`\`typescript
import type { Signal, StateMap } from "./types.ts";

export const signal: Signal = {
  id: "{{id}}",
  name: "{{name}}",
  requires: ["orderbook"],

  evaluate(state: StateMap): boolean {
    return state.orderbook.spread > {{threshold}};
  },

  payload(state: StateMap) {
    return {
      ticker: state.orderbook.ticker,
      spread: state.orderbook.spread,
      bestBid: state.orderbook.bestBid,
      bestAsk: state.orderbook.bestAsk,
    };
  },
};
\`\`\`
```

**Loader:**

```typescript
// src/agent/skills.ts
interface Skill {
  name: string;
  description: string;
  content: string;
}

export async function loadSkills(): Promise<Skill[]> {
  const skills: Skill[] = [];

  for await (const entry of Deno.readDir("./skills")) {
    if (entry.name.endsWith(".md")) {
      const content = await Deno.readTextFile(`./skills/${entry.name}`);
      const { frontmatter, body } = parseFrontmatter(content);
      skills.push({
        name: frontmatter.name,
        description: frontmatter.description,
        content: body,
      });
    }
  }

  return skills;
}
```

### 7. Streaming REPL

```typescript
// src/cli.ts
import { ChatAnthropic } from "@langchain/anthropic";
import { createReactAgent } from "@langchain/langgraph/prebuilt";
import { loadSkills } from "./agent/skills.ts";
import { tools } from "./agent/tools.ts";
import { buildSystemPrompt } from "./agent/prompt.ts";

const model = new ChatAnthropic({
  model: Deno.env.get("SSMD_MODEL") ?? "claude-sonnet-4-20250514",
  streaming: true,
});

const skills = await loadSkills();
const systemPrompt = buildSystemPrompt(skills);

const agent = createReactAgent({
  llm: model,
  tools,
  messageModifier: systemPrompt,
});

const encoder = new TextEncoder();

console.log("ssmd-agent v0.1.0");
console.log("Type 'quit' to exit\n");

while (true) {
  const input = prompt("ssmd-agent>");
  if (!input || input === "quit") break;

  for await (const event of agent.streamEvents(
    { messages: [{ role: "user", content: input }] },
    { version: "v2" }
  )) {
    switch (event.event) {
      case "on_chat_model_stream":
        if (event.data.chunk.content) {
          Deno.stdout.writeSync(encoder.encode(event.data.chunk.content));
        }
        break;
      case "on_tool_start":
        console.log(`\n[tool] ${event.name}(${formatArgs(event.data.input)})`);
        break;
      case "on_tool_end":
        console.log(`  → ${formatResult(event.data.output)}`);
        break;
    }
  }
  console.log("\n");
}
```

## Example Session

```
$ deno task agent

ssmd-agent v0.1.0
Type 'quit' to exit

ssmd-agent> create a spread alert for INXD > 5%

Let me explore what data is available...▌

[tool] list_datasets(feed="kalshi")
  → 3 datasets (2025-12-23 to 2025-12-25, 438K records)

Good, we have recent data. Let me sample INXD orderbook records...▌

[tool] sample_data(feed="kalshi", date="2025-12-25", ticker="INXD", type="orderbook", limit=1000)
  → 1000 records

[tool] orderbook_builder(records)
  → 1000 state snapshots (spread range: 0.02 - 0.08)

I can see spreads vary significantly. I'll generate a signal using the
monitor-spread template with a 5% threshold...▌

--- Generated Signal ---
export const signal: Signal = {
  id: "inxd-spread-alert",
  name: "INXD Wide Spread Alert",
  requires: ["orderbook"],
  evaluate(state) {
    return state.orderbook.ticker.startsWith("INXD")
        && state.orderbook.spreadPercent > 0.05;
  },
  payload(state) {
    return {
      ticker: state.orderbook.ticker,
      spread: state.orderbook.spread,
      spreadPercent: state.orderbook.spreadPercent,
    };
  },
};
------------------------

[tool] run_backtest(signalCode, states)
  → 12 fires, 0 errors

The signal fired 12 times today. The fires are spread across different
times, suggesting real spread widening events rather than noise.

Would you like me to deploy this signal? [y/n]

ssmd-agent> y

[tool] deploy_signal(code, "signals/inxd-spread-alert.ts")
  → Committed: a1b2c3d "signal: add inxd-spread-alert"

Signal deployed to signals/inxd-spread-alert.ts

ssmd-agent> quit
```

## Implementation Order

1. **ssmd-data service** (Go)
   - Extract `internal/dataservice/` from existing `internal/data/`
   - HTTP handlers with API key middleware
   - Docker container

2. **Refactor ssmd CLI** (Go)
   - Update `ssmd data` commands to call service
   - Or keep direct file access for offline use

3. **Agent REPL** (Deno)
   - Skills loader
   - Tool definitions (API clients)
   - LangGraph agent setup
   - Streaming CLI

4. **State builders** (TypeScript)
   - OrderBookBuilder
   - orderbook_builder tool

5. **Backtest runner** (TypeScript)
   - Sandbox signal compilation
   - run_backtest tool

6. **Skills** (Markdown)
   - explore-data
   - interpret-backtest
   - monitor-spread
   - custom-signal

## Dependencies

```json
// deno.json imports
{
  "imports": {
    "@langchain/anthropic": "npm:@langchain/anthropic@^0.3",
    "@langchain/langgraph": "npm:@langchain/langgraph@^0.2",
    "@langchain/core": "npm:@langchain/core@^0.3",
    "zod": "npm:zod@^3.23"
  }
}
```

## Future Work

- **Signal Runtime**: Live NATS subscription, loads signals/*.ts, evaluates in real-time
- **Additional Builders**: PriceHistoryBuilder, VolumeProfileBuilder
- **Memory**: PostgresSaver for cross-session context
- **Auth**: JWT/OAuth for multi-user
- **UI**: TUI (rich terminal) or Web interface
