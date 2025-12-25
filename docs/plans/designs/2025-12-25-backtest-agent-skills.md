# Backtest Component & Agent Skills Design

**Date:** 2025-12-25
**Status:** Draft

## Overview

Design for the backtest component that enables LangGraph agents to generate and validate signal code. Agents use tools to access historical data and skills to know how to work with that data effectively.

## Key Insight

Agents don't run against real-time streams. Instead:
1. Agents generate TypeScript signal code at development time
2. Code is validated against historical/backtest data
3. Validated code deploys to Signal Runtime (no LLM, real-time)

Skills & tools provide the knowledge for generating code that works with streams.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           RUNTIME (Production)                              │
│                                                                             │
│  ┌─────────────┐         ┌─────────────────────────────────────────────┐   │
│  │  Connector  │────────▶│              NATS JetStream                 │   │
│  │   (Rust)    │         │                                             │   │
│  │             │         │  Subjects:                                  │   │
│  │  Kalshi WS  │         │    {env}.kalshi.json.trade.{ticker}        │   │
│  │  → JSON     │         │    {env}.kalshi.json.ticker.{ticker}       │   │
│  │  → Cap'n    │         │    {env}.kalshi.capnp.trade.{ticker}       │   │
│  └─────────────┘         └──────────────┬─────────────────┬────────────┘   │
│                                         │                 │                 │
│                    ┌────────────────────┘                 └────────────┐   │
│                    ▼                                                   ▼   │
│       ┌────────────────────────┐                    ┌──────────────────────┐
│       │    Signal Runtime      │                    │      Archiver        │
│       │    (no LLM, fast)      │                    │       (Rust)         │
│       │                        │                    │                      │
│       │  Loads: signals/*.ts   │                    │  NATS → JSONL.gz     │
│       │  Evaluates conditions  │                    │  Local: /data/ssmd/  │
│       └───────────┬────────────┘                    │  Sync: → GCS         │
│                   │                                 └──────────────────────┘
│                   ▼                                            │
│       {env}.signals.fired.*                                    │
│                                                                │
└────────────────────────────────────────────────────────────────┼────────────┘
                                                                 │
                                            gsutil rsync (cron)  │
                                                                 ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        BACKTEST COMPONENT (new)                             │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                        Dataset Service                                 │ │
│  │                                                                        │ │
│  │  Storage: S3 / Local files (archived NATS data)                        │ │
│  │                                                                        │ │
│  │  Tools:                                                                │ │
│  │    list_datasets(feed?, date_range?) → [{ feed, date, size, tickers }] │ │
│  │    sample_data(feed, date, ticker?, limit) → [records]                 │ │
│  │    get_schema(feed, message_type) → { fields, types }                  │ │
│  │    list_state_builders() → [{ id, description, derived_fields }]       │ │
│  │                                                                        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                        Backtest Runner                                 │ │
│  │                                                                        │ │
│  │  run_backtest(signal_code, feed, date_range, tickers?) →               │ │
│  │    {                                                                   │ │
│  │      fires: 42,                                                        │ │
│  │      errors: 0,                                                        │ │
│  │      fire_times: [...],                                                │ │
│  │      sample_payloads: [...]                                            │ │
│  │    }                                                                   │ │
│  │                                                                        │ │
│  │  Replays archived data through State Builders + Signal Evaluator       │ │
│  │                                                                        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
└──────────────────────────────────────────────────────────────┬──────────────┘
                                                               │
                                                               │ tools
                                                               ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                      DEVELOPMENT TIME (Agent + Skills)                      │
│                                                                             │
│  User: "Alert when spread > 5%"                                             │
│              │                                                              │
│              ▼                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Definition Agent (LangGraph)                        │ │
│  │                                                                        │ │
│  │  TOOLS (access):                 SKILLS (knowledge):                   │ │
│  │                                                                        │ │
│  │  Data Discovery:                 Data Interaction:                     │ │
│  │    - list_datasets                 - explore-data                      │ │
│  │    - sample_data                 Signal Generation:                    │ │
│  │    - get_schema                    - monitor-spread                    │ │
│  │    - list_state_builders           - price-alert                       │ │
│  │                                    - volume-spike                      │ │
│  │  Validation:                       - custom-signal                     │ │
│  │    - run_backtest                Validation:                           │ │
│  │    - validate_signal               - interpret-backtest                │ │
│  │                                                                        │ │
│  │  Deployment:                                                           │ │
│  │    - deploy_signal                                                     │ │
│  │    - reload_runtime                                                    │ │
│  │                                                                        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│              │                                                              │
│              │ generates + validates                                        │
│              ▼                                                              │
│         signals/spread-alert.ts                                             │
│              │                                                              │
│              │ git commit                                                   │
│              ▼                                                              │
│         Signal Runtime reloads                                              │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Components

### 1. Archiver (Rust, subscribes from NATS)

Separate service that subscribes to NATS JetStream and persists data to storage. This decoupling from the connector enables:
- **Sharding**: Multiple archivers can subscribe to different subject patterns
- **Resilience**: Archiver can restart and resume from JetStream position
- **Flexibility**: Connector outputs JSON or Cap'n Proto; archiver writes JSONL

**Data Flow:**
```
Connector ──► NATS JetStream ──► Archiver ──► Local/GCS
              (JSON or Cap'n)     (Rust)       (JSONL.gz)
                                    │
                                    └──► gsutil rsync (homelab → GCS)
```

**NATS Subscription:**
```rust
// Subscribe to JSON stream for archiving (human-readable)
let consumer = stream.create_consumer(ConsumerConfig {
    durable_name: Some("archiver-kalshi".to_string()),
    filter_subject: "prod.kalshi.json.>".to_string(),
    ack_policy: AckPolicy::Explicit,
    max_ack_pending: 1000,
    ..Default::default()
}).await?;
```

**Output format (MVP - JSONL):**
```
/data/ssmd/              # Local storage (homelab)
  kalshi/
    2025-12-25/
      trades.jsonl.gz
      ticker.jsonl.gz
      orderbook.jsonl.gz
      manifest.json

gs://ssmd-data/          # GCS (synced from homelab)
  kalshi/
    2025-12-25/
      ...
```

**GCS Sync (cron on homelab):**
```bash
# Sync completed days to GCS
gsutil -m rsync -r /data/ssmd/ gs://ssmd-data/
```

**Future format (Arrow/Parquet):**
```
/data/ssmd/
  kalshi/
    2025-12-25/
      trades.parquet       # Columnar, efficient for backtest queries
      ticker.parquet
      orderbook.parquet
      manifest.json
```

Arrow/Parquet benefits for backtesting:
- Columnar storage enables efficient field selection
- Predicate pushdown for time-range queries
- Compression built-in (typically 10x better than gzipped JSONL)
- Zero-copy reads with memory mapping
- Native support in Deno via `apache-arrow` npm package

**Manifest:**
```json
{
  "feed": "kalshi",
  "date": "2025-12-25",
  "format": "jsonl",
  "nats_stream": "MARKETDATA",
  "nats_consumer": "archiver-kalshi",
  "last_seq": 1542000,
  "files": [
    { "type": "trades", "records": 15420, "bytes": 2048000 },
    { "type": "ticker", "records": 89000, "bytes": 5120000 },
    { "type": "orderbook", "records": 42000, "bytes": 8192000 }
  ],
  "tickers": ["INXD-25001", "KXBTC-25001", ...],
  "time_range": { "start": "2025-12-25T00:00:00Z", "end": "2025-12-25T23:59:59Z" }
}
```

### 2. Dataset Service (via `ssmd` CLI)

Following the "stupid simple" paradigm, dataset operations are `ssmd data` subcommands. The agent tools shell out to these commands, keeping one CLI for all operations.

**CLI Commands:**

```bash
# List available datasets
ssmd data list [--feed kalshi] [--from 2025-12-20] [--to 2025-12-25]
# Output: JSON array of datasets with date, record count, tickers

# Sample records from a dataset
ssmd data sample <feed> <date> [--ticker INXD-25001] [--limit 10] [--type orderbook]
# Output: JSON array of records

# Show schema for a message type
ssmd data schema <feed> <message_type>
# Output: JSON schema with fields, types, derived fields

# List available state builders
ssmd data builders
# Output: JSON array of state builder definitions
```

**Agent Tool Implementation:**

```typescript
// src/agent/tools.ts
import { tool } from "@langchain/core/tools";
import { z } from "zod";

export const listDatasets = tool(
  async ({ feed, from, to }) => {
    const args = ["data", "list", "--output", "json"];
    if (feed) args.push("--feed", feed);
    if (from) args.push("--from", from);
    if (to) args.push("--to", to);

    const cmd = new Deno.Command("ssmd", { args, stdout: "piped" });
    const { stdout } = await cmd.output();
    return new TextDecoder().decode(stdout);
  },
  {
    name: "list_datasets",
    description: "List available market data datasets",
    schema: z.object({
      feed: z.string().optional().describe("Filter by feed name"),
      from: z.string().optional().describe("Start date (YYYY-MM-DD)"),
      to: z.string().optional().describe("End date (YYYY-MM-DD)"),
    }),
  }
);

export const sampleData = tool(
  async ({ feed, date, ticker, limit, type }) => {
    const args = ["data", "sample", feed, date, "--output", "json"];
    if (ticker) args.push("--ticker", ticker);
    if (limit) args.push("--limit", String(limit));
    if (type) args.push("--type", type);

    const cmd = new Deno.Command("ssmd", { args, stdout: "piped" });
    const { stdout } = await cmd.output();
    return new TextDecoder().decode(stdout);
  },
  {
    name: "sample_data",
    description: "Get sample records from a dataset",
    schema: z.object({
      feed: z.string().describe("Feed name (e.g., kalshi)"),
      date: z.string().describe("Date (YYYY-MM-DD)"),
      ticker: z.string().optional().describe("Filter by ticker"),
      limit: z.number().optional().describe("Max records to return"),
      type: z.string().optional().describe("Message type (trade, orderbook, ticker)"),
    }),
  }
);
```

**Example CLI Output:**

```bash
$ ssmd data list --feed kalshi --from 2025-12-24
[
  { "feed": "kalshi", "date": "2025-12-25", "records": 146420, "tickers": 42, "size_mb": 12.4 },
  { "feed": "kalshi", "date": "2025-12-24", "records": 132100, "tickers": 38, "size_mb": 11.2 }
]

$ ssmd data sample kalshi 2025-12-25 --ticker INXD-25001 --limit 3
[
  { "type": "trade", "ticker": "INXD-25001", "price": 0.55, "size": 100, "ts": 1735084800000 },
  { "type": "trade", "ticker": "INXD-25001", "price": 0.56, "size": 50, "ts": 1735084801000 },
  { "type": "trade", "ticker": "INXD-25001", "price": 0.54, "size": 200, "ts": 1735084802000 }
]

$ ssmd data schema kalshi orderbook
{
  "fields": {
    "ticker": "string",
    "yes_bid": "number",
    "yes_ask": "number",
    "no_bid": "number",
    "no_ask": "number",
    "ts": "number"
  },
  "derived": ["spread", "midpoint", "imbalance"]
}

$ ssmd data builders
[
  { "id": "orderbook", "description": "Maintains bid/ask levels", "derived": ["spread", "bestBid", "bestAsk", "depth"] },
  { "id": "priceHistory", "description": "Rolling price window", "derived": ["vwap", "returns", "volatility"] },
  { "id": "volumeProfile", "description": "Buy/sell volume tracking", "derived": ["buyVolume", "sellVolume", "ratio"] }
]
```

**Entitlements:**

MVP: Everyone is entitled to all data (no auth required).

Future: Entitlement checks based on user/API key:
```yaml
# entitlements.yaml (future)
entitlements:
  - user: "*"              # default: everyone
    feeds: ["kalshi"]
    date_range: "all"

  - user: "backtest-agent"
    feeds: ["kalshi", "polymarket"]
    date_range: "last-30-days"

  - user: "research-team"
    feeds: ["*"]
    date_range: "all"
```

```bash
# MVP: no auth, returns all entitled data
$ ssmd data list --feed kalshi

# Future: user context from environment or flag
$ ssmd data list --feed kalshi --user backtest-agent
# Returns only data user is entitled to access
```

The entitlement check happens in the CLI before accessing storage. This keeps the storage layer simple (no per-file ACLs).

### 3. State Builders (Shared Code)

State builders are TypeScript modules shared between backtest runner and signal runtime. This ensures signals behave identically in testing and production.

**Architecture:**
```
ssmd-agent/
├── src/
│   ├── state/                    # Shared state builder code
│   │   ├── mod.ts                # Exports all builders
│   │   ├── orderbook.ts          # OrderBook state builder
│   │   ├── price-history.ts      # PriceHistory state builder
│   │   └── volume-profile.ts     # VolumeProfile state builder
│   ├── backtest/
│   │   └── runner.ts             # Uses state/ for replay
│   └── runtime/
│       └── evaluator.ts          # Uses state/ for live eval
```

**State Builder Interface:**
```typescript
// src/state/types.ts
export interface StateBuilder<T> {
  id: string;

  // Process a single market data record
  update(record: MarketRecord): void;

  // Get current derived state
  getState(): T;

  // Reset for new ticker/session
  reset(): void;
}

export interface OrderBookState {
  ticker: string;
  bestBid: number;
  bestAsk: number;
  spread: number;
  bidDepth: number;
  askDepth: number;
}
```

**Example Builder:**
```typescript
// src/state/orderbook.ts
import type { StateBuilder, OrderBookState } from "./types.ts";

export class OrderBookBuilder implements StateBuilder<OrderBookState> {
  id = "orderbook";
  private state: OrderBookState = this.initialState();

  update(record: MarketRecord): void {
    if (record.type !== "orderbook") return;
    this.state.ticker = record.ticker;
    this.state.bestBid = record.yes_bid;
    this.state.bestAsk = record.yes_ask;
    this.state.spread = record.yes_ask - record.yes_bid;
    // ... update depths
  }

  getState(): OrderBookState {
    return { ...this.state };
  }

  reset(): void {
    this.state = this.initialState();
  }

  private initialState(): OrderBookState {
    return { ticker: "", bestBid: 0, bestAsk: 0, spread: 0, bidDepth: 0, askDepth: 0 };
  }
}
```

### 4. Backtest Runner

Replays historical data through state builders and evaluates signal code.

**Tool:**

```typescript
run_backtest(
  signal_code: string,      // TypeScript signal code
  feed: string,             // "kalshi"
  date_range: DateRange,    // { start: "2025-12-24", end: "2025-12-25" }
  tickers?: string[]        // optional filter
) → BacktestResult

interface BacktestResult {
  fires: number;            // how many times signal triggered
  errors: string[];         // any runtime errors
  fire_times: string[];     // timestamps of fires
  sample_payloads: object[]; // first N fire payloads
  duration_ms: number;      // how long backtest took
  records_processed: number;
}
```

**Example:**

```typescript
// run_backtest(spreadAlertCode, "kalshi", { start: "2025-12-24", end: "2025-12-25" })
{
  fires: 42,
  errors: [],
  fire_times: ["2025-12-24T14:32:00Z", "2025-12-24T15:01:00Z", ...],
  sample_payloads: [
    { ticker: "INXD-25001", spread: 0.052, bestBid: 0.45, bestAsk: 0.502 },
    { ticker: "INXD-25001", spread: 0.061, bestBid: 0.44, bestAsk: 0.501 },
  ],
  duration_ms: 1234,
  records_processed: 278520
}
```

## Skills

Skills are prompt-driven specializations that teach the agent how to use tools effectively.

### Data Interaction Skills

#### `explore-data`

Teaches the agent how to discover and understand available data.

```markdown
# explore-data

When exploring data for signal development:

1. Start with list_datasets() to see what's available
2. Check date ranges - recent data is more relevant
3. Use sample_data() to understand record structure
4. Look at multiple tickers - patterns may vary
5. Check get_schema() for derived fields you can use

Common patterns:
- Kalshi orderbook has yes_bid/yes_ask (prediction market format)
- Spread = yes_ask - yes_bid
- Trades have price, size, side

Watch out for:
- Gaps in data (market closed, connector issues)
- Low-volume tickers (noisy signals)
- Time zones (all timestamps are UTC)
```

#### `interpret-backtest`

Teaches the agent how to analyze backtest results.

```markdown
# interpret-backtest

When reviewing backtest results:

1. Check errors first - any runtime issues?
2. Fire count: 0 = condition never met, too many = too sensitive
3. Look at fire_times - clustered or spread out?
4. Review sample_payloads - do values make sense?

Iteration patterns:
- fires: 0 → loosen condition (spread > 0.03 instead of > 0.05)
- fires: 1000+ → tighten condition or add cooldown
- errors → check state builder fields exist

Good signals:
- Fire on meaningful events (not noise)
- Reasonable frequency (depends on use case)
- Payload contains actionable info
```

### Signal Generation Skills

#### `monitor-spread`

Template for spread-based signals.

```markdown
# monitor-spread

Generate spread monitoring signals using the orderbook state builder.

Template:
```typescript
import type { Signal, StateMap } from "ssmd-agent/types";

export const signal: Signal = {
  id: "{{id}}",
  name: "{{name}}",
  requires: ["orderbook"],

  evaluate(state: StateMap): boolean {
    const book = state.orderbook;
    return book.spread > {{threshold}};
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
```

Customization options:
- threshold: typically 0.03-0.10 for prediction markets
- Add ticker filter: `&& book.ticker.startsWith("INXD")`
- Add cooldown: track lastFire timestamp
```

#### `price-alert`

Template for price threshold signals.

```markdown
# price-alert

Generate price threshold signals using priceHistory state builder.

Template:
```typescript
import type { Signal, StateMap } from "ssmd-agent/types";

export const signal: Signal = {
  id: "{{id}}",
  name: "{{name}}",
  requires: ["priceHistory"],

  evaluate(state: StateMap): boolean {
    const history = state.priceHistory;
    return history.last {{comparison}} {{threshold}};
  },

  payload(state: StateMap) {
    return {
      ticker: state.priceHistory.ticker,
      price: state.priceHistory.last,
      vwap: state.priceHistory.vwap,
      change: state.priceHistory.returns,
    };
  },
};
```

Comparison options: >, <, >=, <=
For percentage changes, use priceHistory.returns
```

#### `volume-spike`

Template for volume-based signals.

```markdown
# volume-spike

Generate volume spike signals using volumeProfile state builder.

Template:
```typescript
import type { Signal, StateMap } from "ssmd-agent/types";

export const signal: Signal = {
  id: "{{id}}",
  name: "{{name}}",
  requires: ["volumeProfile"],

  evaluate(state: StateMap): boolean {
    const vol = state.volumeProfile;
    return vol.{{metric}} > vol.average * {{multiplier}};
  },

  payload(state: StateMap) {
    return {
      ticker: state.volumeProfile.ticker,
      buyVolume: state.volumeProfile.buyVolume,
      sellVolume: state.volumeProfile.sellVolume,
      ratio: state.volumeProfile.ratio,
    };
  },
};
```

Metrics: buyVolume, sellVolume, totalVolume
Multiplier: typically 2-5x for "spike" detection
```

#### `custom-signal`

Generic signal structure for custom logic.

```markdown
# custom-signal

For custom signals, follow this structure:

```typescript
import type { Signal, StateMap } from "ssmd-agent/types";

export const signal: Signal = {
  id: "unique-kebab-case-id",
  name: "Human Readable Name",

  // Which state builders this signal needs
  requires: ["orderbook", "priceHistory"],  // pick what you need

  // Return true when signal should fire
  evaluate(state: StateMap): boolean {
    // Access state via state.{builder}.{field}
    // Example: state.orderbook.spread, state.priceHistory.vwap
    return /* your condition */;
  },

  // What data to include when signal fires
  payload(state: StateMap) {
    return {
      // Include relevant state for downstream consumers
    };
  },
};
```

Available state builders and their fields:
- orderbook: spread, bestBid, bestAsk, bidDepth, askDepth
- priceHistory: last, vwap, returns, high, low, volatility
- volumeProfile: buyVolume, sellVolume, totalVolume, ratio, average

Combine multiple conditions:
```typescript
evaluate(state) {
  return state.orderbook.spread > 0.05
      && state.volumeProfile.ratio > 2.0;
}
```
```

## Example Agent Flow

```
User: "Create an alert for when Kalshi INXD spread exceeds 5%"

Agent:
  1. Loads skill: explore-data

  2. Calls list_datasets("kalshi")
     → sees data available for 2025-12-24, 2025-12-25

  3. Calls sample_data("kalshi", "2025-12-25", null, 5)
     → understands record format

  4. Calls get_schema("kalshi", "orderbook")
     → confirms spread field exists

  5. Loads skill: monitor-spread
     → gets template for spread-based signals

  6. Generates TypeScript signal:
     - id: "inxd-wide-spread"
     - threshold: 0.05
     - ticker filter: INXD

  7. Calls run_backtest(code, "kalshi", { start: "2025-12-24", end: "2025-12-25" })
     → { fires: 12, errors: [] }

  8. Loads skill: interpret-backtest
     → 12 fires over 2 days seems reasonable

  9. Returns signal code for review

User: "Looks good, deploy it"

Agent:
  10. Calls deploy_signal(code, "signals/inxd-wide-spread.ts")
  11. Git commits the file
  12. Calls reload_runtime()

  → Signal now running in production
```

## CLI Interface

Start with a terminal interface before building UI. Simple REPL that loads skills and runs the agent.

### Command

```bash
# Interactive mode
deno task agent

# Or with initial prompt
deno task agent "create a spread alert for INXD > 5%"
```

### Structure

```
ssmd-agent/
├── src/
│   ├── main.ts              # HTTP health check (existing)
│   ├── cli.ts               # CLI entry point (new)
│   ├── agent/
│   │   ├── graph.ts         # LangGraph definition
│   │   ├── tools.ts         # Dataset/backtest tools
│   │   └── skills.ts        # Skill loader
│   └── skills/
│       ├── explore-data.md
│       ├── interpret-backtest.md
│       ├── monitor-spread.md
│       ├── price-alert.md
│       ├── volume-spike.md
│       └── custom-signal.md
```

### CLI Flow (Streaming Output)

The CLI uses LangGraph streaming to show real-time progress. Tokens stream as the model generates, tool calls show immediately when invoked.

```
$ deno task agent

ssmd-agent v0.1.0
Type 'help' for commands, 'quit' to exit

> create a spread alert for kalshi

I'll help you create a spread alert for Kalshi. Let me first explore
what data is available...▌

[tool] list_datasets("kalshi")
  → Found 3 days of data (2025-12-23 to 2025-12-25)

Good, we have recent data. Let me sample the orderbook structure...▌

[tool] sample_data("kalshi", "2025-12-25", null, 5)
  → Sampled 5 orderbook records

I can see the orderbook has yes_bid/yes_ask fields. I'll generate a
spread monitoring signal using the monitor-spread template...▌

[tool] validate_signal(code)
  → TypeScript: OK

--- Generated Signal ---
// signals/kalshi-spread-alert.ts
import type { Signal, StateMap } from "ssmd-agent/types";

export const signal: Signal = {
  id: "kalshi-spread-alert",
  name: "Kalshi Spread Alert",
  requires: ["orderbook"],
  evaluate(state: StateMap): boolean {
    return state.orderbook.spread > 0.05;
  },
  payload(state: StateMap) {
    return {
      ticker: state.orderbook.ticker,
      spread: state.orderbook.spread,
    };
  },
};
------------------------

Let me run a backtest to validate this signal...▌

[tool] run_backtest(code, "kalshi", {start: "2025-12-24", end: "2025-12-25"})
  → 8 fires, 0 errors

The signal fired 8 times over 2 days, which seems reasonable for a 5%
spread threshold. The fires are spread across different times, not
clustered, indicating real market events rather than noise.

Deploy this signal? [y/n]: y

[tool] deploy_signal(code, "signals/kalshi-spread-alert.ts")
  → Written to signals/kalshi-spread-alert.ts
  → Committed: "signal: add kalshi-spread-alert"

Signal deployed successfully. It will be active on the next runtime reload.

> quit
```

### Streaming Implementation

```typescript
// src/cli.ts
import { ChatAnthropic } from "@langchain/anthropic";
import { createReactAgent } from "@langchain/langgraph/prebuilt";

const model = new ChatAnthropic({
  model: "claude-sonnet-4-20250514",
  streaming: true,
});

const agent = createReactAgent({ llm: model, tools });

// Stream events from the agent
for await (const event of agent.streamEvents(
  { messages: [{ role: "user", content: userInput }] },
  { version: "v2" }
)) {
  if (event.event === "on_chat_model_stream") {
    // Token streaming - write without newline
    const chunk = event.data.chunk;
    if (chunk.content) {
      Deno.stdout.writeSync(new TextEncoder().encode(chunk.content));
    }
  } else if (event.event === "on_tool_start") {
    // Tool invocation
    console.log(`\n[tool] ${event.name}(${JSON.stringify(event.data.input)})`);
  } else if (event.event === "on_tool_end") {
    // Tool result
    console.log(`  → ${formatToolResult(event.data.output)}`);
  }
}
```

**Stream event types:**
| Event | Description |
|-------|-------------|
| `on_chat_model_stream` | Token-by-token output from LLM |
| `on_tool_start` | Tool invocation begins |
| `on_tool_end` | Tool returns result |
| `on_chain_start` | Agent step begins |
| `on_chain_end` | Agent step completes |

### CLI Commands

| Command | Description |
|---------|-------------|
| `help` | Show available commands |
| `datasets` | List available datasets (shortcut for tool) |
| `skills` | List available skills |
| `load <skill>` | Load and display a skill |
| `history` | Show conversation history |
| `clear` | Clear conversation |
| `quit` | Exit |

### deno.json Tasks

```json
{
  "tasks": {
    "start": "deno run --allow-net --allow-env src/main.ts",
    "agent": "deno run --allow-net --allow-env --allow-read --allow-write src/cli.ts",
    "dev": "deno run --watch --allow-net --allow-env src/main.ts",
    "check": "deno check src/main.ts src/cli.ts"
  }
}
```

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `ANTHROPIC_API_KEY` | Yes | For LLM calls |
| `SSMD_DATA_PATH` | No | Path to archived data (default: `./data`) |
| `SSMD_SIGNALS_PATH` | No | Path to signals dir (default: `./signals`) |

## Implementation Order

**Phase 2 (Connector changes):**
1. **Connector refactor** - Remove file writer, NATS-only output, JSON + Cap'n Proto formats
2. **Archiver (Rust)** - NATS subscriber → JSONL.gz, GCS sync

**Phase 3 (Agent pipeline):**
3. **ssmd data CLI (Go)** - `list`, `sample`, `schema`, `builders` commands
4. **State Builders (TypeScript)** - Shared modules for orderbook, priceHistory, volumeProfile
5. **Backtest Runner (TypeScript)** - Replay data through builders, evaluate signals
6. **Skills (Markdown)** - Prompt templates loaded from filesystem
7. **CLI Agent (Deno)** - REPL with streaming, tool wrappers for ssmd
8. **Signal Runtime (TypeScript)** - Live evaluation, NATS subscription

## Decisions

| Question | Decision | Rationale |
|----------|----------|-----------|
| Storage format | JSONL → Arrow/Parquet | JSONL for MVP (simple), migrate to Arrow/Parquet for efficient backtesting |
| Skill loading | File-based | Load from `skills/*.md`, like Claude Code approach |
| State builder sharing | Yes, shared code | Same TypeScript module used by backtest runner and signal runtime |
| Incremental backtest | TBD | Decide when implementing |

## Future Work

### Memory (Post-MVP)

Not needed for MVP (single-session CLI), but will be needed quickly after:

**Why memory matters:**
- Remember user preferences (preferred tickers, threshold defaults)
- Learn from past signal iterations (what worked, what didn't)
- Maintain context across sessions
- Reference previously created signals

**LangGraph memory options:**

| Option | Persistence | Use Case |
|--------|-------------|----------|
| `MemorySaver` | In-memory | Development/testing |
| `PostgresSaver` | PostgreSQL | Production, multi-user |
| `SqliteSaver` | SQLite file | Single-user local |

**Implementation approach:**
```typescript
import { MemorySaver } from "@langchain/langgraph";
import { PostgresSaver } from "@langchain/langgraph-checkpoint-postgres";

// MVP: no persistence
const checkpointer = new MemorySaver();

// Post-MVP: PostgreSQL for cross-session memory
const checkpointer = PostgresSaver.fromConnString(process.env.DATABASE_URL);

const agent = createReactAgent({
  llm: model,
  tools,
  checkpointSaver: checkpointer,
});

// Thread ID determines conversation continuity
const threadId = "user-123-session-456";
const config = { configurable: { thread_id: threadId } };

for await (const event of agent.streamEvents(input, { ...config, version: "v2" })) {
  // ...
}
```

**Thread ID strategies:**
- `session-{uuid}` - New conversation each session (MVP)
- `user-{id}` - Continuous conversation per user
- `user-{id}-signal-{name}` - Context scoped to specific signal work

### Guardrails (Later Stage)

Agent guardrails to be designed in a future iteration:
- Rate limiting on tool calls
- Cost controls for LLM usage
- Validation of generated signal code (AST checks, forbidden patterns)
- Sandbox execution for backtest runner
- Approval workflow before production deploy

## Dependencies

- Archiver requires connector to be running
- Dataset Service requires archived data
- Backtest Runner requires state builders (shared with Signal Runtime)
- Agent requires all of the above + LangGraph.js
