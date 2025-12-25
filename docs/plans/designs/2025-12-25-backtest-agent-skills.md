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

Skills provide the knowledge for generating code that works with streams.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           RUNTIME (Production)                               │
│                                                                              │
│  NATS Streams ──────────────────┬────────────────────────────────────────▶  │
│  kalshi.trade.*                 │                                            │
│  kalshi.ticker.*                │                                            │
│  kalshi.orderbook.*             │                                            │
│                                 │                                            │
│                                 ▼                                            │
│                    ┌────────────────────────┐      ┌──────────────────────┐ │
│                    │    Signal Runtime      │      │      Archiver        │ │
│                    │    (no LLM, fast)      │      │                      │ │
│                    │                        │      │  Writes to S3:       │ │
│                    │  Loads: signals/*.ts   │      │  s3://ssmd-data/     │ │
│                    │  Evaluates conditions  │      │    kalshi/2025-12-25/│ │
│                    └───────────┬────────────┘      │    trades.jsonl.gz   │ │
│                                │                   │    orderbook.jsonl.gz│ │
│                                ▼                   └──────────────────────┘ │
│                    {env}.signals.fired.*                     │              │
│                                                              │              │
└──────────────────────────────────────────────────────────────┼──────────────┘
                                                               │
                                                               │ archived data
                                                               ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        BACKTEST COMPONENT (new)                              │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                        Dataset Service                                  │ │
│  │                                                                         │ │
│  │  Storage: S3 / Local files (archived NATS data)                        │ │
│  │                                                                         │ │
│  │  Tools:                                                                 │ │
│  │    list_datasets(feed?, date_range?) → [{ feed, date, size, tickers }] │ │
│  │    sample_data(feed, date, ticker?, limit) → [records]                  │ │
│  │    get_schema(feed, message_type) → { fields, types }                   │ │
│  │    list_state_builders() → [{ id, description, derived_fields }]       │ │
│  │                                                                         │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                        Backtest Runner                                  │ │
│  │                                                                         │ │
│  │  run_backtest(signal_code, feed, date_range, tickers?) →                │ │
│  │    {                                                                    │ │
│  │      fires: 42,                                                         │ │
│  │      errors: 0,                                                         │ │
│  │      fire_times: [...],                                                 │ │
│  │      sample_payloads: [...]                                             │ │
│  │    }                                                                    │ │
│  │                                                                         │ │
│  │  Replays archived data through State Builders + Signal Evaluator        │ │
│  │                                                                         │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
└──────────────────────────────────────────────────────────────┬──────────────┘
                                                               │
                                                               │ tools
                                                               ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                      DEVELOPMENT TIME (Agent + Skills)                       │
│                                                                              │
│  User: "Alert when spread > 5%"                                              │
│              │                                                               │
│              ▼                                                               │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Definition Agent (LangGraph)                         │ │
│  │                                                                         │ │
│  │  TOOLS (access):                 SKILLS (knowledge):                    │ │
│  │                                                                         │ │
│  │  Data Discovery:                 Data Interaction:                      │ │
│  │    - list_datasets                 - explore-data                       │ │
│  │    - sample_data                 Signal Generation:                     │ │
│  │    - get_schema                    - monitor-spread                     │ │
│  │    - list_state_builders           - price-alert                        │ │
│  │                                    - volume-spike                       │ │
│  │  Validation:                       - custom-signal                      │ │
│  │    - run_backtest                Validation:                            │ │
│  │    - validate_signal               - interpret-backtest                 │ │
│  │                                                                         │ │
│  │  Deployment:                                                            │ │
│  │    - deploy_signal                                                      │ │
│  │    - reload_runtime                                                     │ │
│  │                                                                         │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│              │                                                               │
│              │ generates + validates                                         │
│              ▼                                                               │
│         signals/spread-alert.ts                                              │
│              │                                                               │
│              │ git commit                                                    │
│              ▼                                                               │
│         Signal Runtime reloads                                               │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Components

### 1. Archiver (exists in design, not yet built)

Writes NATS stream data to durable storage for backtesting.

**Output format:**
```
s3://ssmd-data/
  kalshi/
    2025-12-25/
      trades.jsonl.gz
      ticker.jsonl.gz
      orderbook.jsonl.gz
      manifest.json
```

**Manifest:**
```json
{
  "feed": "kalshi",
  "date": "2025-12-25",
  "files": [
    { "type": "trades", "records": 15420, "bytes": 2048000 },
    { "type": "ticker", "records": 89000, "bytes": 5120000 },
    { "type": "orderbook", "records": 42000, "bytes": 8192000 }
  ],
  "tickers": ["INXD-25001", "KXBTC-25001", ...],
  "time_range": { "start": "2025-12-25T00:00:00Z", "end": "2025-12-25T23:59:59Z" }
}
```

### 2. Dataset Service

Provides tools for agents to discover and sample historical data.

**Tools:**

| Tool | Signature | Purpose |
|------|-----------|---------|
| `list_datasets` | `(feed?, date_range?) → Dataset[]` | Discover available data |
| `sample_data` | `(feed, date, ticker?, limit) → Record[]` | Get sample records |
| `get_schema` | `(feed, message_type) → Schema` | Understand data shape |
| `list_state_builders` | `() → StateBuilder[]` | Available derived state |

**Example responses:**

```typescript
// list_datasets("kalshi", { start: "2025-12-20", end: "2025-12-25" })
[
  { feed: "kalshi", date: "2025-12-25", records: 146420, tickers: 42 },
  { feed: "kalshi", date: "2025-12-24", records: 132100, tickers: 38 },
  // ...
]

// sample_data("kalshi", "2025-12-25", "INXD-25001", 3)
[
  { type: "trade", ticker: "INXD-25001", price: 0.55, size: 100, ts: 1735084800000 },
  { type: "trade", ticker: "INXD-25001", price: 0.56, size: 50, ts: 1735084801000 },
  { type: "trade", ticker: "INXD-25001", price: 0.54, size: 200, ts: 1735084802000 },
]

// get_schema("kalshi", "orderbook")
{
  fields: {
    ticker: "string",
    yes_bid: "number",
    yes_ask: "number",
    no_bid: "number",
    no_ask: "number",
    ts: "number"
  },
  derived: ["spread", "midpoint", "imbalance"]
}

// list_state_builders()
[
  { id: "orderbook", description: "Maintains bid/ask levels", derived: ["spread", "bestBid", "bestAsk", "depth"] },
  { id: "priceHistory", description: "Rolling price window", derived: ["vwap", "returns", "volatility"] },
  { id: "volumeProfile", description: "Buy/sell volume tracking", derived: ["buyVolume", "sellVolume", "ratio"] },
]
```

### 3. Backtest Runner

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

### CLI Flow

```
$ deno task agent

ssmd-agent v0.1.0
Type 'help' for commands, 'quit' to exit

> create a spread alert for kalshi

[Agent] Loading skill: explore-data
[Agent] Calling list_datasets("kalshi")
  → Found 3 days of data (2025-12-23 to 2025-12-25)

[Agent] Calling sample_data("kalshi", "2025-12-25", null, 5)
  → Sampled 5 orderbook records

[Agent] Loading skill: monitor-spread
[Agent] Generating signal code...

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

[Agent] Running backtest on 2025-12-25...
  → 8 fires, 0 errors

Deploy this signal? [y/n]: y

[Agent] Written to signals/kalshi-spread-alert.ts
[Agent] Committed: "signal: add kalshi-spread-alert"

> quit
```

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

1. **Archiver** - Record NATS data to S3/local files
2. **Dataset Service** - Tools for list/sample/schema
3. **Backtest Runner** - Replay + evaluate signals
4. **Skills** - Prompt templates (markdown files)
5. **CLI** - Terminal REPL interface
6. **Definition Agent** - LangGraph integration

## Open Questions

1. **Storage format** - JSONL (simple) vs Parquet (efficient) vs Cap'n Proto (matches runtime)?
2. **Skill loading** - File-based like Claude Code or database?
3. **State builder sharing** - Same code for backtest and runtime?
4. **Incremental backtest** - Stream results or wait for completion?

## Dependencies

- Archiver requires connector to be running
- Dataset Service requires archived data
- Backtest Runner requires state builders (shared with Signal Runtime)
- Agent requires all of the above + LangGraph.js
