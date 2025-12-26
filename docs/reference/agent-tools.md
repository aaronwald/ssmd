# ssmd-agent Tools and State Reference

This document describes the tools and state builders available in ssmd-agent.

## Tools vs Skills

| Aspect | Tools | Skills |
|--------|-------|--------|
| **Format** | TypeScript with Zod schemas | Markdown text |
| **Structure** | Typed inputs/outputs, validation | Unstructured prose |
| **Purpose** | Execute actions (API calls, file writes) | Guide LLM reasoning/approach |
| **Invocation** | LLM calls function with parameters | LLM reads as system prompt context |
| **Location** | `src/agent/tools.ts` | `skills/*.md` |

**When to use each:**
- **Tools**: When the agent needs to fetch data, compute something, or perform an action
- **Skills**: When the agent needs guidance on *how* to approach a task or interpret results

---

## Tools

### Data Discovery Tools

#### `list_datasets`

List available market data datasets.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `feed` | string | No | Filter by feed name (e.g., `"kalshi"`) |
| `from` | string | No | Start date `YYYY-MM-DD` |
| `to` | string | No | End date `YYYY-MM-DD` |

**Returns:** Array of dataset info objects

```json
[
  {
    "feed": "kalshi",
    "date": "2025-12-25",
    "records": 45230,
    "tickers": 127,
    "size_mb": 12.5,
    "has_gaps": false
  }
]
```

---

#### `list_tickers`

List all tickers available in a dataset for a given feed and date.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `feed` | string | Yes | Feed name (e.g., `"kalshi"`) |
| `date` | string | Yes | Date `YYYY-MM-DD` |

**Returns:** Array of ticker strings

```json
["INXD-25001", "BTCUSD-25003", "ETHUSD-25004"]
```

---

#### `sample_data`

Get sample records from a dataset.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `feed` | string | Yes | Feed name (e.g., `"kalshi"`) |
| `date` | string | Yes | Date `YYYY-MM-DD` |
| `ticker` | string | No | Filter by ticker symbol |
| `type` | string | No | Message type: `trade`, `ticker`, `orderbook` |
| `limit` | number | No | Max records (default 10) |

**Returns:** Array of raw market records

```json
[
  {
    "type": "ticker",
    "ticker": "INXD-25001",
    "yes_bid": 0.45,
    "yes_ask": 0.55,
    "no_bid": 0.45,
    "no_ask": 0.55,
    "ts": 1735084800000
  }
]
```

---

#### `get_schema`

Get schema for a message type.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `feed` | string | Yes | Feed name |
| `type` | string | Yes | Message type: `trade`, `ticker`, `orderbook` |

**Returns:** Schema with field types and derived fields

```json
{
  "type": "ticker",
  "fields": {
    "ticker": "string",
    "yes_bid": "number",
    "yes_ask": "number",
    "ts": "number"
  },
  "derived": ["spread", "midpoint"]
}
```

---

#### `list_builders`

List available state builders for signal development.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| (none) | | | |

**Returns:** Array of builder descriptions

```json
[
  {
    "id": "orderbook",
    "description": "Maintains bid/ask levels from orderbook updates",
    "derived": ["spread", "bestBid", "bestAsk", "midpoint"]
  }
]
```

---

### State Building Tools

#### `orderbook_builder`

Process market records through the OrderBook state builder.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `records` | array | Yes | Array of market data records from `sample_data` |

**Returns:** State snapshots with spread calculations

```json
{
  "count": 50,
  "snapshots": [
    {
      "ticker": "INXD-25001",
      "bestBid": 0.45,
      "bestAsk": 0.55,
      "spread": 0.10,
      "spreadPercent": 0.182,
      "lastUpdate": 1735084800000
    }
  ],
  "summary": {
    "ticker": "INXD-25001",
    "spreadRange": { "min": 0.05, "max": 0.15 }
  }
}
```

**Note:** Limited to 100 snapshots in response to prevent oversized payloads.

---

### Validation Tools

#### `run_backtest`

Evaluate signal code against state snapshots.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `signalCode` | string | Yes | TypeScript signal code with `evaluate()` and `payload()` functions |
| `states` | array | Yes | OrderBookState snapshots from `orderbook_builder` |

**Returns:** Backtest results

```json
{
  "fires": 12,
  "total": 100,
  "fireRate": 0.12,
  "errors": [],
  "samplePayloads": [
    { "ticker": "INXD-25001", "spread": 0.12 }
  ]
}
```

**Signal code format:**

```typescript
export const signal = {
  id: "my-signal",
  name: "My Signal",
  requires: ["orderbook"],

  evaluate(state: { orderbook: OrderBookState }): boolean {
    return state.orderbook.spread > 0.05;
  },

  payload(state: { orderbook: OrderBookState }) {
    return {
      ticker: state.orderbook.ticker,
      spread: state.orderbook.spread,
    };
  },
};
```

---

### Deployment Tools

#### `deploy_signal`

Write signal file and git commit.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `code` | string | Yes | Complete TypeScript signal code |
| `path` | string | Yes | Filename within `signals/` directory (e.g., `"spread-alert.ts"`) |

**Returns:** Deployment result

```json
{
  "path": "./signals/spread-alert.ts",
  "sha": "a1b2c3d",
  "message": "Deployed to ./signals/spread-alert.ts"
}
```

**Note:** This tool performs a git commit. Use only after successful backtest.

---

## State Builders

State builders process raw market records into structured state that signals can evaluate.

### OrderBookBuilder

Maintains current bid/ask state from orderbook or ticker messages.

**Input:** `MarketRecord` with type `orderbook` or `ticker`

**Output:** `OrderBookState`

| Field | Type | Description |
|-------|------|-------------|
| `ticker` | string | Symbol identifier |
| `bestBid` | number | Current best bid (yes_bid) |
| `bestAsk` | number | Current best ask (yes_ask) |
| `spread` | number | `bestAsk - bestBid` |
| `spreadPercent` | number | `spread / bestAsk` (0 if bestAsk is 0) |
| `lastUpdate` | number | Timestamp (Unix ms) of last update |

**Example usage in signal:**

```typescript
evaluate(state: { orderbook: OrderBookState }): boolean {
  // Fire when spread exceeds 5%
  return state.orderbook.spreadPercent > 0.05;
}
```

---

### Future Builders (Not Yet Implemented)

| Builder | Description | Derived Fields |
|---------|-------------|----------------|
| `priceHistory` | Rolling window of price history | `last`, `vwap`, `returns`, `high`, `low`, `volatility` |
| `volumeProfile` | Buy/sell volume tracking | `buyVolume`, `sellVolume`, `totalVolume`, `ratio` |

---

## Market Record Types

Raw records from the data API have this structure:

### Common Fields

| Field | Type | Description |
|-------|------|-------------|
| `type` | string | Message type: `trade`, `ticker`, `orderbook` |
| `ticker` | string | Symbol identifier |
| `ts` | number | Timestamp (Unix milliseconds, UTC) |

### Ticker/Orderbook Fields

| Field | Type | Description |
|-------|------|-------------|
| `yes_bid` | number | Best bid for YES outcome |
| `yes_ask` | number | Best ask for YES outcome |
| `no_bid` | number | Best bid for NO outcome |
| `no_ask` | number | Best ask for NO outcome |

### Trade Fields

| Field | Type | Description |
|-------|------|-------------|
| `price` | number | Execution price |
| `count` | number | Trade count/size |
| `side` | string | `"yes"` or `"no"` |
| `taker_side` | string | Taker side |

---

## Skills

Skills are markdown files that provide context and guidance to the LLM.

| Skill | Purpose |
|-------|---------|
| `explore-data` | How to discover and understand available data |
| `monitor-spread` | Guidance for building spread monitoring signals |
| `interpret-backtest` | How to analyze backtest results |
| `custom-signal` | Template and guidance for custom signal logic |

Skills are loaded from `skills/*.md` and injected into the system prompt.

---

## Typical Workflow

1. **Discover** → `list_datasets()` to see what's available
2. **Sample** → `sample_data(feed, date)` to examine records
3. **Schema** → `get_schema(feed, type)` to understand fields
4. **Build State** → `orderbook_builder(records)` to create snapshots
5. **Backtest** → `run_backtest(signalCode, states)` to validate
6. **Iterate** → Adjust thresholds based on fire rate
7. **Deploy** → `deploy_signal(code, path)` when satisfied
