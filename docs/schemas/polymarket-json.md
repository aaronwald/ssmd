# Polymarket WebSocket JSON Schema Reference

**Last Updated**: 2026-02-15

This document defines every JSON message type received from the Polymarket CLOB
WebSocket API and processed by the ssmd connector. It covers field-level
definitions, identifier semantics, array/fan-out implications, and the
relationship to secmaster tables.

**Source of truth**:
- Rust structs: `ssmd-rust/crates/connector/src/polymarket/messages.rs`
- Polymarket CLOB docs: https://docs.polymarket.com/developers/CLOB/websocket/market-channel

---

## Table of Contents

1. [Connection and Subscription](#connection-and-subscription)
2. [Identifier Model](#identifier-model)
3. [Message Types](#message-types)
   - [last_trade_price](#last_trade_price)
   - [book](#book)
   - [price_change](#price_change)
   - [best_bid_ask](#best_bid_ask)
   - [new_market](#new_market)
   - [market_resolved](#market_resolved)
   - [tick_size_change](#tick_size_change)
4. [Control Messages](#control-messages)
5. [Market Discovery (Gamma REST API)](#market-discovery-gamma-rest-api)
6. [NATS Subject Routing](#nats-subject-routing)
7. [Archiver Validation Rules](#archiver-validation-rules)
8. [Fan-Out Implications for Parquet](#fan-out-implications-for-parquet)

---

## Connection and Subscription

### WebSocket Endpoint

```
wss://ws-subscriptions-clob.polymarket.com/ws/market
```

No authentication is required for the public market data channel.

### Subscription Message

Sent by the connector after opening the WebSocket connection:

```json
{
  "assets_ids": [
    "21742633143463906290569050155826241533067272736897614950488156847949938836455",
    "48331043336612883890938759509493159234755048973440709113929715223126545928914"
  ],
  "type": "market",
  "custom_feature_enabled": true
}
```

| Field | Type | Description |
|-------|------|-------------|
| `assets_ids` | `string[]` | Token IDs (not condition IDs) to subscribe to |
| `type` | `string` | Always `"market"` for the public market channel |
| `custom_feature_enabled` | `boolean` | Enables `best_bid_ask`, `new_market`, `market_resolved` events |

### Connection Limits

| Parameter | Value | Notes |
|-----------|-------|-------|
| Max instruments per connection | 500 | Exceeding causes instability; connector shards at this limit |
| Concurrent connections | ~10 | Polymarket throttles beyond ~10 WS connections |
| Max message size | 2 MiB | Configured in connector for large book snapshots |
| Read timeout | 120 seconds | No data received = dead connection, connector exits |

### Keepalive

Polymarket uses **app-level text PING**, not WebSocket-level ping frames:

- Connector sends raw string `PING` every **10 seconds**
- Server responds with raw string `PONG`
- `PONG` responses are filtered out before NATS publish

There is no subscription confirmation message. After subscribing, the server
immediately begins sending `book` snapshots for subscribed instruments.

### Message Envelope

Polymarket sends messages in two formats:

1. **Single JSON object** (most common after initial subscription):
   ```json
   {"event_type":"last_trade_price","asset_id":"...","market":"0x...","price":"0.55"}
   ```

2. **Array-wrapped JSON** (observed during bursts and initial book snapshots):
   ```json
   [{"event_type":"book","asset_id":"...","market":"0x...","bids":[...],"asks":[...]}]
   ```

The connector writer handles both: it first attempts single-object deserialization
(fast path), then falls back to array parsing and publishes each element
individually to NATS.

---

## Identifier Model

Polymarket uses a three-level identifier hierarchy. Understanding this is
critical for mapping WebSocket data to secmaster tables.

### Hierarchy

```
Event (top-level question grouping)
 └── Condition (a specific market/question)
      ├── YES Token (outcome token)
      └── NO Token (outcome token)
```

### Identifier Fields in WebSocket Messages

| Field | WS Key | Format | Example | Secmaster Mapping |
|-------|--------|--------|---------|-------------------|
| Condition ID | `market` | Hex string, `0x`-prefixed, 64 hex chars | `0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af` | `polymarket_conditions.condition_id` |
| Token ID | `asset_id` | Large decimal integer string, 70+ digits | `21742633143463906290569050155826241533067272736897614950488156847949938836455` | `polymarket_tokens.token_id` |

**Key relationships**:

- **`market`** = `condition_id` in secmaster. Identifies the market/question.
  Used for NATS subject routing (shorter than token IDs). Present in every
  message type. All messages for both YES and NO tokens of the same condition
  share the same `market` value.

- **`asset_id`** = `token_id` in secmaster. Identifies a specific outcome token
  (YES or NO). Used for WebSocket subscription. Present in `book`,
  `last_trade_price`, `best_bid_ask`, `tick_size_change`, and inside
  `price_changes[]` items. A single `price_change` message can contain items for
  multiple `asset_id` values (both YES and NO tokens of the same condition).

### Secmaster Tables

```sql
-- polymarket_conditions (one row per market)
condition_id    VARCHAR(128) PRIMARY KEY  -- = WS "market" field
question        TEXT
slug            VARCHAR(256)
category        VARCHAR(128)
tags            TEXT[]                    -- GIN indexed, used for category filtering
status          VARCHAR(16)              -- 'active', 'resolved', etc.
active          BOOLEAN
volume          NUMERIC(24,2)
liquidity       NUMERIC(24,2)

-- polymarket_tokens (two rows per condition: YES and NO)
token_id        VARCHAR(128) PRIMARY KEY  -- = WS "asset_id" field
condition_id    VARCHAR(128) REFERENCES polymarket_conditions
outcome         VARCHAR(128)              -- "Yes" or "No"
outcome_index   INTEGER                   -- 0 or 1
price           NUMERIC(8,4)
bid             NUMERIC(8,4)
ask             NUMERIC(8,4)
volume          NUMERIC(24,2)
```

---

## Message Types

All prices are **decimal strings** (e.g., `"0.55"`). All timestamps are
**Unix milliseconds as strings** (e.g., `"1706000000000"`). Most fields are
optional (nullable) except where noted.

### last_trade_price

**Exchange docs**: [Market Channel](https://docs.polymarket.com/developers/CLOB/websocket/market-channel)

Trade execution event. Triggered when a maker and taker order match.

**NATS routing**: `{prefix}.json.trade.{condition_id}`

```json
{
  "event_type": "last_trade_price",
  "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
  "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
  "price": "0.547",
  "side": "BUY",
  "size": "100",
  "fee_rate_bps": "0",
  "timestamp": "1707300000000"
}
```

| Field | JSON Type | Required | Nullable | Description |
|-------|-----------|----------|----------|-------------|
| `event_type` | string | Yes | No | Always `"last_trade_price"` |
| `asset_id` | string | Yes | No | Token ID (YES or NO token that was traded) |
| `market` | string | Yes | No | Condition ID (`0x`-prefixed hex) |
| `price` | string | Yes | No | Trade price as decimal string, range 0.00-1.00 |
| `side` | string | No | Yes | Taker side: `"BUY"` or `"SELL"` |
| `size` | string | No | Yes | Trade size (number of contracts) |
| `fee_rate_bps` | string | No | Yes | Fee rate in basis points (e.g., `"0"`, `"200"`) |
| `timestamp` | string | No | Yes | Unix milliseconds |

**Validation required fields** (archiver): `asset_id`, `market`, `price`

**Parquet fan-out**: 1:1 (one JSONL line = one parquet row)

---

### book

**Exchange docs**: [Market Channel](https://docs.polymarket.com/developers/CLOB/websocket/market-channel)

Full orderbook snapshot. Emitted immediately upon subscription and whenever
trades change the orderbook. This is typically the largest message type.

**NATS routing**: `{prefix}.json.orderbook.{condition_id}`

```json
{
  "event_type": "book",
  "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
  "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
  "timestamp": "1706000000000",
  "hash": "abc123def456",
  "bids": [
    {"price": "0.55", "size": "1000"},
    {"price": "0.54", "size": "500"}
  ],
  "asks": [
    {"price": "0.56", "size": "750"},
    {"price": "0.57", "size": "300"}
  ]
}
```

| Field | JSON Type | Required | Nullable | Description |
|-------|-----------|----------|----------|-------------|
| `event_type` | string | Yes | No | Always `"book"` |
| `asset_id` | string | Yes | No | Token ID for this orderbook side |
| `market` | string | Yes | No | Condition ID |
| `timestamp` | string | No | Yes | Unix milliseconds |
| `hash` | string | No | Yes | Orderbook summary hash (for change detection) |
| `bids` | OrderbookLevel[] | Yes | No | Buy-side price levels (alias: `buys`) |
| `asks` | OrderbookLevel[] | Yes | No | Sell-side price levels (alias: `sells`) |

**OrderbookLevel**:

| Field | JSON Type | Required | Description |
|-------|-----------|----------|-------------|
| `price` | string | Yes | Price level as decimal string |
| `size` | string | Yes | Aggregate size at this price level |

**Field aliases**: The Polymarket API sends `buys`/`sells` in some messages.
The connector deserializes both `buys`/`bids` and `sells`/`asks` via serde aliases.

**Validation required fields** (archiver): `asset_id`, `market`

**Parquet fan-out**: 1:N (one JSONL line with N bid levels + M ask levels fans
out to N+M parquet rows in an orderbook schema, or stored as a single snapshot
row with nested arrays depending on schema version)

**Note on initial book snapshots**: Some initial book snapshots arrive
**without** the `event_type` field. The writer detects these by checking for the
presence of `bids`/`asks` fields and routes them to the orderbook subject.

---

### price_change

**Exchange docs**: [Market Channel](https://docs.polymarket.com/developers/CLOB/websocket/market-channel)

Incremental orderbook price level update. Triggered by new orders or
cancellations that change a price level.

**NATS routing**: `{prefix}.json.ticker.{condition_id}`

```json
{
  "event_type": "price_change",
  "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
  "timestamp": "1706000000000",
  "price_changes": [
    {
      "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
      "price": "0.55",
      "size": "750",
      "side": "BUY",
      "hash": "order123abc",
      "best_bid": "0.55",
      "best_ask": "0.56"
    },
    {
      "asset_id": "48331043336612883890938759509493159234755048973440709113929715223126545928914",
      "price": "0.45",
      "size": "750",
      "side": "SELL",
      "hash": "order456def",
      "best_bid": "0.44",
      "best_ask": "0.45"
    }
  ]
}
```

| Field | JSON Type | Required | Nullable | Description |
|-------|-----------|----------|----------|-------------|
| `event_type` | string | Yes | No | Always `"price_change"` |
| `market` | string | Yes | No | Condition ID |
| `timestamp` | string | No | Yes | Unix milliseconds |
| `price_changes` | PriceChangeItem[] | Yes | No | Array of individual level changes |

**Note**: No top-level `asset_id` — the `asset_id` is inside each
`price_changes` item. A single `price_change` message can contain updates for
both the YES and NO tokens of a condition.

**PriceChangeItem**:

| Field | JSON Type | Required | Nullable | Description |
|-------|-----------|----------|----------|-------------|
| `asset_id` | string | Yes | No | Token ID affected by this price change |
| `price` | string | Yes | No | Price level that changed |
| `size` | string | Yes | No | New aggregate size at this level (`"0"` = level removed) |
| `side` | string | Yes | No | `"BUY"` or `"SELL"` |
| `hash` | string | No | Yes | Order hash that triggered the change |
| `best_bid` | string | No | Yes | Updated best bid after this change |
| `best_ask` | string | No | Yes | Updated best ask after this change |

**Validation required fields** (archiver): `market`, `price_changes` (non-empty),
plus per-item: `asset_id`, `price`, `size`, `side`

**Parquet fan-out**: 1:N (one JSONL line with N `price_changes` items fans out
to N parquet rows). This is the most common source of fan-out in the Polymarket
data pipeline.

---

### best_bid_ask

**Exchange docs**: [Market Channel](https://docs.polymarket.com/developers/CLOB/websocket/market-channel)

Top-of-book quote update. Emitted when the best bid or ask price changes.
Requires `custom_feature_enabled: true` in the subscription message.

**NATS routing**: `{prefix}.json.ticker.{condition_id}`

```json
{
  "event_type": "best_bid_ask",
  "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
  "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
  "best_bid": "0.55",
  "best_ask": "0.56",
  "spread": "0.01",
  "timestamp": "1706000000000"
}
```

| Field | JSON Type | Required | Nullable | Description |
|-------|-----------|----------|----------|-------------|
| `event_type` | string | Yes | No | Always `"best_bid_ask"` |
| `market` | string | Yes | No | Condition ID |
| `asset_id` | string | Yes | No | Token ID |
| `best_bid` | string | No | Yes | Current best bid price |
| `best_ask` | string | No | Yes | Current best ask price |
| `spread` | string | No | Yes | Bid-ask spread |
| `timestamp` | string | No | Yes | Unix milliseconds |

**Validation required fields** (archiver): `market`, `asset_id`

**Parquet fan-out**: 1:1

---

### new_market

**Exchange docs**: [Market Channel](https://docs.polymarket.com/developers/CLOB/websocket/market-channel)

Notification that a new market (condition) has been created. Requires
`custom_feature_enabled: true`.

**NATS routing**: `{prefix}.json.lifecycle.{condition_id}`

```json
{
  "event_type": "new_market",
  "market": "0x5678efgh90abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
  "question": "Will Bitcoin reach $150K by end of 2026?",
  "slug": "will-bitcoin-reach-150k-by-end-of-2026",
  "assets_ids": [
    "71321045223419920183048303847283947283940193847562738490123456789012345678901",
    "98765432109876543210987654321098765432109876543210987654321098765432109876543"
  ],
  "outcomes": ["Yes", "No"],
  "timestamp": "1706000000000"
}
```

| Field | JSON Type | Required | Nullable | Description |
|-------|-----------|----------|----------|-------------|
| `event_type` | string | Yes | No | Always `"new_market"` |
| `market` | string | Yes | No | Condition ID for the new market |
| `question` | string | No | Yes | Human-readable market question |
| `slug` | string | No | Yes | URL-friendly slug |
| `assets_ids` | string[] | No | No (defaults to `[]`) | Token IDs for each outcome |
| `outcomes` | string[] | No | No (defaults to `[]`) | Outcome labels (typically `["Yes", "No"]`) |
| `timestamp` | string | No | Yes | Unix milliseconds |

**Note**: `assets_ids` (plural with underscore) matches the Polymarket API
field name. The array is positionally aligned with `outcomes` — `assets_ids[0]`
corresponds to `outcomes[0]`.

**Parquet fan-out**: 1:1 (lifecycle events are low volume)

---

### market_resolved

**Exchange docs**: [Market Channel](https://docs.polymarket.com/developers/CLOB/websocket/market-channel)

Notification that a market has been resolved (settled). The winning outcome
and token are identified. Requires `custom_feature_enabled: true`.

**NATS routing**: `{prefix}.json.lifecycle.{condition_id}`

```json
{
  "event_type": "market_resolved",
  "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
  "winning_asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
  "winning_outcome": "Yes",
  "timestamp": "1706000000000"
}
```

| Field | JSON Type | Required | Nullable | Description |
|-------|-----------|----------|----------|-------------|
| `event_type` | string | Yes | No | Always `"market_resolved"` |
| `market` | string | Yes | No | Condition ID |
| `winning_asset_id` | string | No | Yes | Token ID of the winning outcome |
| `winning_outcome` | string | No | Yes | Winning outcome label (e.g., `"Yes"`, `"No"`) |
| `timestamp` | string | No | Yes | Unix milliseconds |

**Resolution mechanism**: Most Polymarket markets resolve via UMA (Universal
Market Access) optimistic oracle. Disputed resolutions go through UMA's dispute
process.

**Parquet fan-out**: 1:1

---

### tick_size_change

**Exchange docs**: [Market Channel](https://docs.polymarket.com/developers/CLOB/websocket/market-channel)

Notification that the minimum tick size has changed for a token. Polymarket
adjusts tick sizes when prices approach extreme values (above 0.96 or below
0.04).

**NATS routing**: Not published. The connector **skips** this message type.

```json
{
  "event_type": "tick_size_change",
  "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
  "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
  "old_tick_size": "0.01",
  "new_tick_size": "0.001",
  "side": "BUY",
  "timestamp": "1706000000000"
}
```

| Field | JSON Type | Required | Nullable | Description |
|-------|-----------|----------|----------|-------------|
| `event_type` | string | Yes | No | Always `"tick_size_change"` |
| `asset_id` | string | Yes | No | Token ID |
| `market` | string | Yes | No | Condition ID |
| `old_tick_size` | string | No | Yes | Previous minimum tick size |
| `new_tick_size` | string | No | Yes | New minimum tick size |
| `side` | string | No | Yes | Which side changed: `"BUY"` or `"SELL"` |
| `timestamp` | string | No | Yes | Unix milliseconds |

**Not archived**: The connector writer skips `tick_size_change` events with a
trace log. They do not reach NATS or the archiver.

---

## Control Messages

### PONG

Server response to the connector's `PING` keepalive. Raw text string, not JSON.

```
PONG
```

Filtered out at two levels:
1. Connector receiver: skips `PONG` before sending to mpsc channel
2. Writer: skips messages where `data == b"PONG"`

### No Subscription Confirmation

Unlike Kalshi and Kraken, Polymarket does **not** send a subscription
confirmation message. After the subscribe message is sent, the server
immediately begins streaming `book` snapshots for subscribed instruments.

### Error Messages

The Polymarket API may send error messages, but the connector does not have a
dedicated error message type. Unrecognized `event_type` values fall through to
the "unknown" case in `extract_event_type()` and are counted in metrics as
`inc_message("unknown")`.

---

## Market Discovery (Gamma REST API)

Token IDs for WebSocket subscription are sourced through a priority chain:

```
1. POLYMARKET_TOKEN_IDS env var (static, comma-separated)
       │ if empty:
       ▼
2. Secmaster API (category-based filtering)
   GET {secmaster_url}/v1/polymarket/tokens?category=Crypto&minVolume=100000
       │ if not configured:
       ▼
3. Gamma REST API fallback
   GET https://gamma-api.polymarket.com/markets?active=true&closed=false
```

### Gamma API Response

```
GET https://gamma-api.polymarket.com/markets?active=true&closed=false&limit=100&offset=0
```

Response (array of market objects):

```json
[
  {
    "conditionId": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
    "clobTokenIds": "[\"21742633143463...\", \"48331043336612...\"]",
    "question": "Will Bitcoin reach $100K?",
    "active": true,
    "closed": false
  }
]
```

**Gamma API gotchas**:

| Issue | Detail |
|-------|--------|
| `clobTokenIds` is a **stringified** JSON array | Value is `"[\"id1\", \"id2\"]"` not `["id1", "id2"]`. The connector handles both formats. |
| `category` field on `/markets` is always `null` | Deprecated. Use `/events` endpoint with `tags[]` for category filtering. |
| Pagination is offset/limit based | Max 100 per page. No cursor-based pagination. |
| No max-pages cap by default | The connector paginate until a page returns < 100 results. |

### Secmaster Filtering

When using secmaster-based discovery, the connector filters by:

- **Categories**: `tags TEXT[]` column with GIN index (e.g., `Crypto`)
- **Min volume**: `POLYMARKET_MIN_VOLUME` env var (e.g., `100000`)
- **Question keywords**: `POLYMARKET_QUESTION_FILTER` env var (e.g., `bitcoin,btc,ethereum,eth`)

Current production configuration: `categories: [Crypto]` yields ~272 tokens
across ~136 conditions, fitting in 1 shard.

---

## NATS Subject Routing

The connector uses `condition_id` (the `market` field) for NATS subject routing.
This is shorter than token IDs and naturally groups YES/NO token data for the
same market condition.

### Subject Format

```
{env}.{feed}.json.{type}.{condition_id}
```

### Subject Mapping

| Event Type | NATS Subject Type | Example Subject |
|------------|-------------------|-----------------|
| `last_trade_price` | `trade` | `prod.polymarket.json.trade.0xbd31dc8a...` |
| `price_change` | `ticker` | `prod.polymarket.json.ticker.0xbd31dc8a...` |
| `best_bid_ask` | `ticker` | `prod.polymarket.json.ticker.0xbd31dc8a...` |
| `book` | `orderbook` | `prod.polymarket.json.orderbook.0xbd31dc8a...` |
| `new_market` | `lifecycle` | `prod.polymarket.json.lifecycle.0x5678efgh...` |
| `market_resolved` | `lifecycle` | `prod.polymarket.json.lifecycle.0xbd31dc8a...` |
| `tick_size_change` | *(skipped)* | Not published to NATS |

**Note**: `price_change` and `best_bid_ask` both route to `ticker`. This means
the archiver's JSONL files for the `ticker` NATS subject type contain a mix of
both message types, distinguished by `event_type`.

### NATS Stream

```
Stream:   PROD_POLYMARKET
Subjects: prod.polymarket.>
Max Size: 512 MB
Max Age:  48 hours
Dupe:     2 min window
Storage:  file
```

---

## Archiver Validation Rules

The archiver performs lightweight field-presence validation on Polymarket
messages. Validation uses minimal serde structs (borrowed `&str`) to avoid full
JSON tree allocation.

### Manifest Field Extraction

For every message, the archiver extracts:
- `event_type` (from `event_type` field) -> `msg_type` in manifest
- `market` (condition ID) -> `ticker` in manifest

### Per-Type Required Fields

| Event Type | Required Fields | Notes |
|------------|----------------|-------|
| `book` | `asset_id`, `market` | `bids`/`asks` not validated (may be empty) |
| `last_trade_price` | `asset_id`, `market`, `price` | `side`, `size`, `timestamp` optional |
| `price_change` | `market`, `price_changes` (non-empty array) | First item validated: `asset_id`, `price`, `size`, `side` |
| `best_bid_ask` | `market`, `asset_id` | `best_bid`, `best_ask`, `spread` optional |
| Other types | *(no validation)* | `new_market`, `market_resolved`, `tick_size_change` pass through |

**Validation is sampled** (1 in 100 messages) and **non-blocking**: missing
fields are logged as warnings but the message is still written to JSONL.

---

## Fan-Out Implications for Parquet

When converting JSONL to parquet, some message types produce multiple rows from
a single JSON line. Understanding fan-out is critical for DQ record count
reconciliation.

| Message Type | Fan-Out | Detail |
|-------------|---------|--------|
| `last_trade_price` | 1:1 | One trade = one parquet row |
| `book` | 1:N | One snapshot with N bids + M asks = N+M level rows (or 1 snapshot row with nested arrays) |
| `price_change` | 1:N | One message with N `price_changes` items = N parquet rows |
| `best_bid_ask` | 1:1 | One quote update = one parquet row |
| `new_market` | 1:1 | Rare lifecycle event |
| `market_resolved` | 1:1 | Rare lifecycle event |

**DQ implication**: When verifying `records_by_type` from the manifest against
parquet row counts, `price_change` and `book` message types will show parquet
row counts >= JSONL line counts due to fan-out. DQ must account for this
asymmetry.
