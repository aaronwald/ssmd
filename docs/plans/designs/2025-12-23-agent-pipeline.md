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

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              ssmd (existing, Rust)                          │
│                                                                             │
│  ┌─────────────┐     ┌─────────────┐     ┌──────────────────────────────┐  │
│  │   Kalshi    │────▶│  Connector  │────▶│         NATS JetStream       │  │
│  │  WebSocket  │     │             │     │                              │  │
│  └─────────────┘     └─────────────┘     │  kalshi.trades.>             │  │
│                                          │  kalshi.orderbook.>          │  │
│                                          │  kalshi.ticker.>             │  │
│                                          │                              │  │
│                                          │  {env}.signals.>  ◀────────────────┐
│                                          └───────────┬──────────────────┘  │  │
└──────────────────────────────────────────────────────┼─────────────────────┘  │
                                                       │                        │
                     ┌─────────────────────────────────┼────────────────────────┼─┐
                     │                    ssmd-agent (Deno)                     │ │
                     │                                 │                        │ │
                     │                                 ▼                        │ │
                     │  ┌─────────────────────────────────────────────────┐    │ │
                     │  │                   State Builders                 │    │ │
                     │  │  ┌──────────────┐  ┌──────────────┐  ┌────────┐ │    │ │
                     │  │  │  OrderBook   │  │ PriceHistory │  │ Volume │ │    │ │
                     │  │  │  Builder     │  │   Builder    │  │Profile │ │    │ │
                     │  │  └──────────────┘  └──────────────┘  └────────┘ │    │ │
                     │  └──────────────────────────┬──────────────────────┘    │ │
                     │                             │                           │ │
                     │                             ▼ (derived state)           │ │
                     │  ┌─────────────────────────────────────────────────┐    │ │
                     │  │                  Signal Runtime                  │    │ │
                     │  │  ┌────────────────┐  ┌────────────────┐         │    │ │
                     │  │  │ spread-alert   │  │ depth-imbalance│  ...    │    │ │
                     │  │  └───────┬────────┘  └───────┬────────┘         │    │ │
                     │  └──────────┼───────────────────┼──────────────────┘    │ │
                     │             └─────────┬─────────┘                       │ │
                     │                       ▼ (signal fires)                  │ │
                     │  ┌─────────────────────────────────────────────────┐    │ │
                     │  │              Action Agent (LangGraph.js)        │────┘ │
                     │  │  - Interprets fired signal                      │      │
                     │  │  - Decides action: alert, log, webhook, trade   │      │
                     │  │  - Publishes SignalEvent to NATS                │      │
                     │  └─────────────────────────────────────────────────┘      │
                     │                                                           │
                     │  ┌─────────────────────────────────────────────────┐      │
                     │  │           Definition Agent (LangGraph.js)       │      │
                     │  │  User: "Alert when spread > 5%"                 │      │
                     │  │           │                                     │      │
                     │  │           ▼                                     │      │
                     │  │  Understand → Generate TS → Validate → Deploy   │      │
                     │  └─────────────────────────────────────────────────┘      │
                     │                                                           │
                     │  signals/                    state/                       │
                     │    spread-alert.ts             orderbook.ts               │
                     │    depth-imbalance.ts          price-history.ts           │
                     │    (version controlled)        (version controlled)       │
                     └───────────────────────────────────────────────────────────┘
                                                       │
                                                       ▼ (archived)
                     ┌───────────────────────────────────────────────────────────┐
                     │  S3: signals/2025/12/23/spread-alert/events.jsonl.gz      │
                     └───────────────────────────────────────────────────────────┘
```

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

### Definition Agent

LangGraph.js graph for creating signals from natural language:

```typescript
const graph = new StateGraph<AgentState>()
  .addNode("understand", understandIntent)    // parse user request
  .addNode("generate", generateSignalCode)    // LLM generates TS
  .addNode("validate", validateSignal)        // type-check, lint
  .addNode("confirm", confirmWithUser)        // show code, ask approval
  .addNode("deploy", deploySignal)            // write to signals/, reload
  .compile();
```

Generated code is committed to git for version control and auditability.

### Action Agent

LangGraph.js graph invoked when signals fire:

- Receives SignalEvent with full context
- Interprets why the signal fired
- Decides action: alert, log, webhook, or trade signal
- Publishes SignalEvent to NATS for persistence

## Technology Choices

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Runtime | Deno | Native TypeScript, sandboxed execution for generated code |
| Agent framework | LangGraph.js | Official TypeScript port, same patterns as Python |
| NATS client | nats.deno | Official Deno support |
| Signal code | TypeScript | Same language for definition and execution |

## Data Flow

1. **Kalshi → Connector → NATS** (existing ssmd)
2. **NATS → State Builders** (order book, price history)
3. **State → Signal Runtime** (evaluate conditions, no LLM)
4. **Signal fires → Action Agent** (LLM interprets, decides action)
5. **SignalEvent → NATS → S3** (audit trail)

**Signal creation flow:**

6. **User chat → Definition Agent → generates .ts → git commit → runtime reloads**

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
