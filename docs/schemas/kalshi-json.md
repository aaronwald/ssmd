# Kalshi WebSocket JSON Message Schemas

**Last Updated**: 2026-02-15

This document describes every JSON message type received from the Kalshi WebSocket
API as handled by the ssmd connector. It covers the raw wire format, field
definitions, units, sequence numbering, and how our connector/archiver/parquet
pipeline interprets each field.

**Source of truth**: Kalshi AsyncAPI spec at `https://docs.kalshi.com/asyncapi.yaml`
and our connector code at `ssmd-rust/crates/connector/src/kalshi/messages.rs`.

### Exchange Properties

| Property | Value |
|----------|-------|
| **Sequenced** | Partially — `seq` on trade and orderbook channels only (per-subscription scope). Ticker has `Clock` (global monotonic, not sequential). |
| **Gap detection** | Trade only, via `exchange_seq` column in parquet (per-ticker GROUP BY). Ticker and lifecycle have no usable sequence. |
| **Identifier** | `market_ticker` — human-readable string (e.g. `KXBTCD-26FEB07-T98000`) |
| **Timestamps** | Unix seconds (UTC) |
| **Price units** | Cents (0-99 for yes/no bid/ask) |

---

## Envelope Structure

All Kalshi WebSocket messages share a common envelope with a `type` discriminator:

```json
{
  "type": "<message_type>",
  "sid": <subscription_id>,
  "seq": <sequence_number>,
  "msg": { ... }
}
```

| Field | JSON Type | Description | Present On |
|-------|-----------|-------------|------------|
| `type` | string | Message type discriminator (see sections below) | All messages |
| `sid` | integer | Subscription ID assigned by Kalshi on subscribe | Data messages, lifecycle |
| `seq` | integer | Per-subscription sequence number | Some data messages (trade, orderbook) |
| `id` | integer | Command ID echoed from subscribe/unsubscribe request | Control messages only |
| `msg` | object | Payload (structure varies by type) | Data and some control messages |

The `type` field is used by `serde(tag = "type")` in our Rust enum `WsMessage` to
route deserialization to the correct variant.

---

## Data Messages

### Ticker (`type: "ticker"`)

**Exchange docs**: [Market Ticker](https://docs.kalshi.com/websockets/market-ticker.md)
**Channel**: `ticker`
**Scope**: All markets (global subscribe) or filtered by `market_tickers`
**Frequency**: On every quote/volume change for subscribed markets

#### Full JSON Example (captured from production)

```json
{
  "type": "ticker",
  "sid": 1,
  "msg": {
    "market_id": "2ee24704-7e13-4248-97f5-3b7ffabf4325",
    "market_ticker": "KXRANKLISTGOOGLESEARCH-26JAN-BIA",
    "price": 12,
    "yes_bid": 11,
    "yes_ask": 12,
    "price_dollars": "0.1200",
    "yes_bid_dollars": "0.1100",
    "yes_ask_dollars": "0.1200",
    "volume": 351970,
    "open_interest": 182646,
    "dollar_volume": 175985,
    "dollar_open_interest": 91323,
    "ts": 1732579880,
    "Clock": 6598272994
  }
}
```

#### Field Definitions

**Envelope fields:**

| Field | JSON Type | Required | Description |
|-------|-----------|----------|-------------|
| `type` | string | yes | Always `"ticker"` |
| `sid` | integer | yes | Subscription ID |

**Payload (`msg`) fields:**

| Field | JSON Type | Nullable | Units | Description |
|-------|-----------|----------|-------|-------------|
| `market_ticker` | string | no | — | Market identifier (e.g., `KXBTCD-26FEB14-T100000`) |
| `market_id` | string (UUID) | no | — | Internal Kalshi market UUID |
| `yes_bid` | integer | yes | cents (1-99) | Best bid price for YES contracts |
| `yes_ask` | integer | yes | cents (1-99) | Best ask price for YES contracts |
| `no_bid` | integer | yes | cents (1-99) | Best bid price for NO contracts |
| `no_ask` | integer | yes | cents (1-99) | Best ask price for NO contracts |
| `price` | integer | yes | cents (0-99) | Last trade price (aliased as `last_price` in connector) |
| `volume` | integer | yes | contracts | Total contracts traded |
| `open_interest` | integer | yes | contracts | Current open interest |
| `price_dollars` | string | yes | dollars | Last price as dollar string (e.g., `"0.1200"`) |
| `yes_bid_dollars` | string | yes | dollars | Best bid as dollar string |
| `yes_ask_dollars` | string | yes | dollars | Best ask as dollar string |
| `dollar_volume` | integer | yes | dollars | Dollar volume |
| `dollar_open_interest` | integer | yes | dollars | Dollar open interest |
| `ts` | integer | no | Unix seconds (UTC) | Exchange timestamp |
| `Clock` | integer | yes | opaque | Exchange-assigned monotonic clock value |

**Notes:**
- `price` can be `0` for markets with no trades yet (observed in production).
- `price_dollars` can be empty string `""` when price is 0.
- Our connector deserializes `price` into `last_price` via `#[serde(alias = "price")]`.
- `Clock` is a Kalshi-internal monotonic value (not a timestamp). It is preserved
  in parquet as `exchange_clock` for ordering analysis.
- Fields not captured by our `TickerData` struct (`market_id`, `*_dollars`,
  `dollar_*`) are present in the raw JSON archived to JSONL but not parsed
  by the connector. They are available in the raw archive for replay.

#### Connector Handling

The connector's `TickerData` struct captures:
```rust
pub struct TickerData {
    pub market_ticker: String,       // Required
    pub yes_bid: Option<i64>,        // Nullable
    pub yes_ask: Option<i64>,        // Nullable
    pub no_bid: Option<i64>,         // Nullable
    pub no_ask: Option<i64>,         // Nullable
    pub last_price: Option<i64>,     // Nullable, aliased from "price"
    pub volume: Option<i64>,         // Nullable
    pub open_interest: Option<i64>,  // Nullable
    pub ts: DateTime<Utc>,           // Required, deserialized from Unix seconds
}
```

All other fields in the raw JSON are ignored by the connector but preserved in
the JSONL archive (raw pass-through).

---

### Trade (`type: "trade"`)

**Exchange docs**: [Public Trades](https://docs.kalshi.com/websockets/public-trades.md)
**Channel**: `trade`
**Scope**: All markets (global subscribe) or filtered by `market_tickers`
**Frequency**: On every trade execution

#### Full JSON Example (from Kalshi API spec)

```json
{
  "type": "trade",
  "sid": 1,
  "seq": 42,
  "msg": {
    "trade_id": "f851595a-abcd-4321-9876-1234567890ab",
    "market_ticker": "KXBTCD-26FEB14-T100000",
    "yes_price": 55,
    "no_price": 45,
    "yes_price_dollars": "0.5500",
    "no_price_dollars": "0.4500",
    "count": 10,
    "count_fp": "10.00",
    "taker_side": "yes",
    "ts": 1707667200
  }
}
```

#### Field Definitions

**Envelope fields:**

| Field | JSON Type | Required | Description |
|-------|-----------|----------|-------------|
| `type` | string | yes | Always `"trade"` |
| `sid` | integer | yes | Subscription ID |
| `seq` | integer | sometimes | Per-subscription sequence number (not always present) |

**Payload (`msg`) fields:**

| Field | JSON Type | Nullable | Units | Description |
|-------|-----------|----------|-------|-------------|
| `trade_id` | string (UUID) | no | — | Unique trade identifier |
| `market_ticker` | string | no | — | Market identifier |
| `yes_price` | integer | no | cents (1-99) | YES side price. **Canonical field name from Kalshi WS.** |
| `no_price` | integer | yes | cents (1-99) | NO side price (`100 - yes_price`) |
| `yes_price_dollars` | string | yes | dollars | YES price as dollar string |
| `no_price_dollars` | string | yes | dollars | NO price as dollar string |
| `count` | integer | no | contracts | Number of contracts traded |
| `count_fp` | string | yes | contracts | Fixed-point count (2 decimals) |
| `taker_side` | string | no | — | `"yes"` or `"no"` — which side was the taker |
| `ts` | integer | no | Unix seconds (UTC) | Trade execution timestamp |

**Field Aliases (important):**

The Kalshi WS API has evolved its field names. Our code handles both variants:

| Kalshi WS Field (current) | Legacy/Alternative | Connector Alias |
|---------------------------|-------------------|-----------------|
| `yes_price` | `price` | `#[serde(alias = "yes_price")]` on `price` field |
| `taker_side` | `side` | `#[serde(alias = "taker_side")]` on `side` field |

The parquet schema (`KalshiTradeSchema`) also handles both:
```rust
// Tries "yes_price" first, falls back to "price"
msg.get("yes_price").or_else(|| msg.get("price"))
// Tries "taker_side" first, falls back to "side"
msg.get("taker_side").or_else(|| msg.get("side"))
```

**Sequence number (`seq`):**
- Present at the **envelope level** (not inside `msg`).
- Per-subscription sequence (scoped to the `sid`).
- Extracted by parquet-gen as `exchange_seq` column.
- May be absent in some messages — parquet column is nullable.

#### Connector Handling

```rust
pub struct TradeData {
    pub market_ticker: String,  // Required
    pub price: i64,             // Required, aliased from "yes_price"
    pub count: i64,             // Required
    pub side: String,           // Required, aliased from "taker_side"
    pub ts: DateTime<Utc>,      // Required, from Unix seconds
}
```

Note: `trade_id` is **not** in the connector's `TradeData` struct but **is**
required by the archiver validation and parquet schema. It exists in the raw
JSON pass-through.

---

### Market Lifecycle V2 (`type: "market_lifecycle_v2"`)

**Exchange docs**: [Market & Event Lifecycle](https://docs.kalshi.com/websockets/market-&-event-lifecycle.md)
**Channel**: `market_lifecycle_v2`
**Scope**: All markets (global, no filtering)
**Frequency**: On market state transitions

#### Full JSON Example

```json
{
  "type": "market_lifecycle_v2",
  "sid": 13,
  "msg": {
    "market_ticker": "KXBTCD-26JAN2310-T105000",
    "event_type": "activated",
    "open_ts": 1737554400,
    "close_ts": 1737558000,
    "additional_metadata": {
      "settlement_value": null,
      "name": "Bitcoin above $105,000?",
      "title": "Bitcoin Price",
      "rules": "...",
      "event_ticker": "KXBTCD-26JAN2310",
      "expected_expiration_ts": 1737558000,
      "strike_type": "greater",
      "floor_strike": 105000,
      "cap_strike": null
    }
  }
}
```

#### Minimal Example (created event)

```json
{
  "type": "market_lifecycle_v2",
  "sid": 13,
  "msg": {
    "market_ticker": "KXBTCD-26JAN2310-T105000",
    "event_type": "created"
  }
}
```

#### Field Definitions

**Envelope fields:**

| Field | JSON Type | Required | Description |
|-------|-----------|----------|-------------|
| `type` | string | yes | Always `"market_lifecycle_v2"` |
| `sid` | integer | yes | Subscription ID |

**Payload (`msg`) fields:**

| Field | JSON Type | Nullable | Units | Description |
|-------|-----------|----------|-------|-------------|
| `market_ticker` | string | no | — | Market identifier |
| `event_type` | string | no | — | Lifecycle event (see values below) |
| `open_ts` | integer | yes | Unix seconds (UTC) | When market opens for trading |
| `close_ts` | integer | yes | Unix seconds (UTC) | When market closes for trading |
| `result` | string | yes | — | Market result (on `determined` event) |
| `determination_ts` | integer | yes | Unix seconds (UTC) | When result was determined |
| `settled_ts` | integer | yes | Unix seconds (UTC) | When settlement completed |
| `is_deactivated` | boolean | yes | — | Whether trading is paused |
| `additional_metadata` | object | yes | — | Extra context (on `created`, varies) |

**`event_type` values:**

| Value | Description | Notable Fields |
|-------|-------------|----------------|
| `created` | Market created | `open_ts`, `close_ts`, `additional_metadata` |
| `activated` | Trading opened | `open_ts`, `close_ts` |
| `deactivated` | Trading paused | `is_deactivated` |
| `close_date_updated` | Close time changed | `close_ts` (new value) |
| `determined` | Outcome resolved | `result`, `determination_ts` |
| `settled` | Payouts complete | `settled_ts` |

#### Connector Handling

```rust
pub struct MarketLifecycleData {
    pub market_ticker: String,                          // Required
    pub event_type: String,                             // Required
    pub open_ts: Option<DateTime<Utc>>,                 // Optional, from Unix seconds
    pub close_ts: Option<DateTime<Utc>>,                // Optional, from Unix seconds
    pub additional_metadata: Option<serde_json::Value>, // Optional, preserved as JSON
}
```

---

### Event Lifecycle (`type: "event_lifecycle"`)

**Exchange docs**: [Market & Event Lifecycle](https://docs.kalshi.com/websockets/market-&-event-lifecycle.md)
**Channel**: `market_lifecycle_v2` (shared channel — both market and event lifecycle events arrive here)
**Scope**: All events (global)
**Frequency**: When parent events are created

#### Full JSON Example

```json
{
  "type": "event_lifecycle",
  "sid": 14,
  "msg": {
    "event_ticker": "KXBTCD-26JAN2310",
    "title": "Bitcoin Price",
    "sub_title": "Will BTC exceed $105,000?",
    "collateral_return_type": "MECNET",
    "series_ticker": "KXBTCD",
    "strike_date": 1737558000
  }
}
```

#### Field Definitions

**Envelope fields:**

| Field | JSON Type | Required | Description |
|-------|-----------|----------|-------------|
| `type` | string | yes | Always `"event_lifecycle"` |
| `sid` | integer | yes | Subscription ID |

**Payload (`msg`) fields:**

| Field | JSON Type | Nullable | Units | Description |
|-------|-----------|----------|-------|-------------|
| `event_ticker` | string | no | — | Parent event identifier (e.g., `KXBTCD-26JAN2310`) |
| `title` | string | yes | — | Event title |
| `sub_title` | string | yes | — | Event subtitle/question |
| `collateral_return_type` | string | yes | — | `"MECNET"`, `"DIRECNET"`, or empty |
| `series_ticker` | string | yes | — | Series this event belongs to (e.g., `KXBTCD`) |
| `strike_date` | integer | yes | Unix seconds (UTC) | Event target date |
| `strike_period` | string | yes | — | Period description |

#### Connector Handling

```rust
pub struct EventLifecycleData {
    pub event_ticker: String,                      // Required
    pub title: Option<String>,                     // Optional
    pub sub_title: Option<String>,                 // Optional
    pub collateral_return_type: Option<String>,    // Optional
    pub series_ticker: Option<String>,             // Optional
    pub strike_date: Option<DateTime<Utc>>,        // Optional, from Unix seconds
}
```

**Note**: Event lifecycle messages are forwarded to NATS but do not currently
have a dedicated parquet schema. They are preserved in JSONL archives.

---

### Orderbook Snapshot (`type: "orderbook_snapshot"`)

**Exchange docs**: [Orderbook Updates](https://docs.kalshi.com/websockets/orderbook-updates.md)
**Channel**: `orderbook_delta` (initial message after subscribing)
**Scope**: Per-market subscription only (requires `market_ticker`)
**Frequency**: Once per subscription, then deltas

#### JSON Example

```json
{
  "type": "orderbook_snapshot",
  "sid": 5,
  "seq": 1,
  "msg": {
    "market_ticker": "KXBTCD-26FEB14-T100000",
    "market_id": "abc-123-uuid",
    "yes": [[45, 100], [46, 250], [47, 50]],
    "no": [[53, 120], [54, 300]],
    "yes_dollars": [["0.45", 100], ["0.46", 250]],
    "no_dollars": [["0.53", 120], ["0.54", 300]]
  }
}
```

#### Field Definitions

**Payload (`msg`) fields:**

| Field | JSON Type | Nullable | Units | Description |
|-------|-----------|----------|-------|-------------|
| `market_ticker` | string | no | — | Market identifier |
| `market_id` | string (UUID) | no | — | Kalshi internal UUID |
| `yes` | array of `[price, qty]` | yes | cents, contracts | YES side levels |
| `no` | array of `[price, qty]` | yes | cents, contracts | NO side levels |
| `yes_dollars` | array | yes | dollars, contracts | YES side in dollar format |
| `no_dollars` | array | yes | dollars, contracts | NO side in dollar format |

**Sequence fields**: Both `sid` and `seq` are present. `seq` starts at 1 for the
snapshot and increments with each subsequent delta.

---

### Orderbook Delta (`type: "orderbook_delta"`)

**Exchange docs**: [Orderbook Updates](https://docs.kalshi.com/websockets/orderbook-updates.md)
**Channel**: `orderbook_delta` (incremental updates after snapshot)
**Scope**: Per-market subscription
**Frequency**: On every orderbook change

#### JSON Example

```json
{
  "type": "orderbook_delta",
  "sid": 5,
  "seq": 42,
  "msg": {
    "market_ticker": "KXBTCD-26FEB14-T100000",
    "market_id": "abc-123-uuid",
    "price": 46,
    "price_dollars": "0.46",
    "delta": -50,
    "delta_fp": "-50.00",
    "side": "yes",
    "ts": "2026-02-14T15:30:00Z"
  }
}
```

#### Field Definitions

**Payload (`msg`) fields:**

| Field | JSON Type | Nullable | Units | Description |
|-------|-----------|----------|-------|-------------|
| `market_ticker` | string | no | — | Market identifier |
| `market_id` | string (UUID) | no | — | Kalshi internal UUID |
| `price` | integer | no | cents (1-99) | Price level that changed |
| `price_dollars` | string | no | dollars | Price as dollar string |
| `delta` | integer | no | contracts | Change in quantity (positive = added, negative = removed) |
| `delta_fp` | string | no | contracts | Fixed-point delta |
| `side` | string | no | — | `"yes"` or `"no"` |
| `client_order_id` | string | yes | — | Present only if user's own order caused the change |
| `ts` | string | yes | RFC3339 UTC | Timestamp of change |

**Important**: Orderbook delta requires maintaining state from the initial snapshot.
Each delta modifies the book at a specific `(side, price)` level. `seq` ensures
ordering.

#### Connector Handling (shared struct)

```rust
pub struct OrderbookData {
    pub market_ticker: String,
    pub yes: Option<Vec<(i64, i64)>>,  // (price_cents, quantity)
    pub no: Option<Vec<(i64, i64)>>,
}
```

**Note**: The connector uses the same `OrderbookData` struct for both snapshot and
delta, which works for the snapshot's array format. Individual delta fields
(`price`, `delta`, `side`) are preserved in the raw JSON pass-through but
not parsed into the struct. Our connectors currently do not subscribe to
orderbook channels in production (only ticker + trade + lifecycle).

---

## Control Messages

### Subscribed (`type: "subscribed"`)

Confirmation that a subscription was created (primarily for `orderbook_delta`).

```json
{
  "type": "subscribed",
  "id": 1,
  "msg": {
    "channel": "ticker",
    "sid": 42
  }
}
```

| Field | JSON Type | Description |
|-------|-----------|-------------|
| `id` | integer | Echoed command ID from subscribe request |
| `msg.channel` | string | Channel name subscribed to |
| `msg.sid` | integer | Subscription ID for future messages |

**Note**: Older Kalshi API responses may omit `msg` entirely:
```json
{"type": "subscribed", "id": 1}
```

### Ok (`type: "ok"`)

Confirmation for ticker/trade channel subscriptions and update_subscription commands.

```json
{
  "id": 123,
  "sid": 456,
  "seq": 222,
  "type": "ok",
  "market_tickers": ["MARKET-1", "MARKET-2", "MARKET-3"]
}
```

| Field | JSON Type | Nullable | Description |
|-------|-----------|----------|-------------|
| `id` | integer | no | Echoed command ID |
| `sid` | integer | yes | Subscription ID |
| `seq` | integer | yes | Current sequence position |
| `market_tickers` | array of string | yes | List of confirmed tickers |

**Note**: Unlike `subscribed`, the `ok` response has `sid`, `seq`, and
`market_tickers` at the **top level**, not nested inside `msg`. This reflects
a different confirmation path in Kalshi's API for bulk subscriptions.

### Error (`type: "error"`)

Error response from the exchange.

```json
{
  "id": 123,
  "type": "error",
  "msg": {
    "code": 6,
    "msg": "Already subscribed"
  }
}
```

| Field | JSON Type | Nullable | Description |
|-------|-----------|----------|-------------|
| `id` | integer | yes | Echoed command ID (if applicable) |
| `msg.code` | integer | no | Error code (1-22) |
| `msg.msg` | string | no | Human-readable error message |

**Common error codes:**

| Code | Meaning |
|------|---------|
| 2 | Invalid parameters |
| 6 | Already subscribed |
| 7 | Not subscribed |
| 10 | Authentication required |

### Unsubscribed (`type: "unsubscribed"`)

Confirmation that a subscription was removed.

```json
{
  "id": 5,
  "sid": 42,
  "seq": 100,
  "type": "unsubscribed"
}
```

### Unknown Messages

Any message with an unrecognized `type` is deserialized as `WsMessage::Unknown`
and logged at `warn` level. The connector does **not** forward unknown messages
to NATS.

---

## Subscription Commands (Outgoing)

### Subscribe

```json
{
  "id": 1,
  "cmd": "subscribe",
  "params": {
    "channels": ["ticker"],
    "market_tickers": ["KXBTCD-26FEB14-T100000", "KXBTCD-26FEB14-T95000"]
  }
}
```

| Field | JSON Type | Required | Description |
|-------|-----------|----------|-------------|
| `id` | integer | yes | Client-assigned command ID (monotonically increasing) |
| `cmd` | string | yes | `"subscribe"` |
| `params.channels` | array of string | yes | Channel(s) to subscribe to |
| `params.market_ticker` | string | no | Single market (for per-market channels) |
| `params.market_tickers` | array of string | no | Multiple markets (max 256) |

**Our connector uses**:
- Global subscribe (no tickers): for `ticker`, `trade`, `market_lifecycle_v2`
- Filtered subscribe (with `market_tickers`): for category-filtered connectors
- Per-market subscribe (with `market_ticker`): for `orderbook_delta`

### Unsubscribe

```json
{
  "id": 5,
  "cmd": "unsubscribe",
  "params": {
    "sids": [42, 43]
  }
}
```

### Update Subscription

```json
{
  "id": 6,
  "cmd": "update_subscription",
  "params": {
    "sids": [42],
    "market_tickers": ["KXBTCD-26FEB15-T100000"],
    "action": "add_markets"
  }
}
```

---

## Sequence Numbers and Ordering

### Sequence Number Fields

There are three distinct sequence/ordering fields in Kalshi messages:

| Field | Location | Scope | Type | Description |
|-------|----------|-------|------|-------------|
| `sid` | envelope | per-connection | integer | Subscription ID assigned at subscribe time. Identifies which subscription produced this message. |
| `seq` | envelope | per-subscription | integer | Monotonically increasing per `sid`. Present on trade and orderbook channels. Used for ordering within a subscription. |
| `Clock` | `msg` (ticker only) | global | integer | Kalshi-internal monotonic clock. Observed values in billions range (e.g., `6598272994`). Useful for global ordering across subscriptions. |

### Scope and Guarantees

- **`sid`** is assigned by Kalshi when you subscribe and is stable for the
  lifetime of that subscription. Different subscriptions (even on the same
  channel) get different `sid` values.

- **`seq`** increments per-subscription (per-`sid`). After reconnection, a new
  subscription gets a new `sid` and `seq` restarts. The connector does not
  track `seq` for gap detection — that role belongs to NATS JetStream sequences.

- **`Clock`** appears only in ticker messages, inside the `msg` payload. It is
  a global ordering value across all markets. Our parquet schema captures it as
  `exchange_clock` (Int64, nullable). Not present in trade or lifecycle messages.

### How Our Pipeline Uses Sequences

```
Exchange side:
  sid   → Not stored in parquet. Used only during subscription management.
  seq   → Stored as "exchange_seq" in trade parquet (Int64, nullable).
  Clock → Stored as "exchange_clock" in ticker parquet (Int64, nullable).

Pipeline side (not from Kalshi):
  NATS stream_sequence → Stored as "_nats_seq" in all parquet schemas (UInt64).
                          Used for gap detection and dedup in DQ.
  _received_at         → Archiver reception timestamp (microseconds UTC).
```

---

## Timestamp Fields

All Kalshi timestamps in WebSocket messages follow these conventions:

| Field | Format | Precision | Timezone |
|-------|--------|-----------|----------|
| `ts` (in `msg`) | Unix integer | seconds | UTC |
| `time` (in ticker) | RFC3339 string | varies | UTC |
| `open_ts` | Unix integer | seconds | UTC |
| `close_ts` | Unix integer | seconds | UTC |
| `strike_date` | Unix integer | seconds | UTC |
| `determination_ts` | Unix integer | seconds | UTC |
| `settled_ts` | Unix integer | seconds | UTC |
| `ts` (in orderbook delta) | RFC3339 string | varies | UTC |

Our connector deserializes `ts` from Unix seconds using a custom deserializer:
```rust
pub fn deserialize_unix_timestamp<'de, D>(d: D) -> Result<DateTime<Utc>, D::Error> {
    let ts = i64::deserialize(d)?;
    DateTime::from_timestamp(ts, 0).ok_or_else(|| /* error */)
}
```

In parquet, all timestamps are stored as `Timestamp(Microsecond, UTC)` — the
seconds value is multiplied by `1_000_000`.

---

## Price Units

Kalshi provides prices in multiple formats. Our pipeline uses **cents**:

| Format | Example | Where Used |
|--------|---------|------------|
| Cents (integer 1-99) | `55` | `yes_bid`, `yes_ask`, `price`, `yes_price` — **used by our pipeline** |
| Dollars (string) | `"0.5500"` | `price_dollars`, `yes_bid_dollars` — ignored by connector |
| Centi-cents (integer) | `550000` | `position_cost`, `realized_pnl` in `market_positions` channel — **not used** |
| Fixed-point (string) | `"10.00"` | `count_fp`, `volume_fp` — ignored by connector |

**Conversion**: `cents / 100 = dollars`. A price of `55` = $0.55 = 55% implied probability.

**Special case**: Price `0` is valid (observed in production for markets with
no trades). `price_dollars` may be empty string `""` in this case.

---

## Symbol/Identifier Fields

### Market Ticker (`market_ticker`)

The primary identifier for a market contract. Format varies by product:

| Series | Ticker Format | Example |
|--------|--------------|---------|
| Daily above/below | `{SERIES}-{DDMMMHHMM}-T{STRIKE}` | `KXBTCD-26FEB0620-T72999.99` |
| Daily bracket/range | `{SERIES}-{DDMMMHHMM}-B{BRACKET}` | `KXBTC-26FEB0620-B79875` |
| 15-min directional | `{SERIES}-{DDMMMHHMMSS}-{MIN}` | `KXBTC15M-26FEB081915-15` |
| Weekly max | `{SERIES}-{DDMMM}-W{STRIKE}` | `KXBTCMAXW-26FEB14-W120000` |
| Sports/Other | Varies | `KXRANKLISTGOOGLESEARCH-26JAN-BIA` |

**Mapping to secmaster**: `market_ticker` maps directly to the `ticker` column
in the ssmd `markets` table. The event ticker (prefix before the last `-T`/`-B`
segment) maps to the `events` table.

### Event Ticker (`event_ticker`)

Parent event identifier. One event contains multiple markets:
- Event: `KXBTCD-26JAN2310` (Bitcoin price on Jan 23 10:00)
- Markets: `KXBTCD-26JAN2310-T105000`, `KXBTCD-26JAN2310-T100000`, etc.

### Series Ticker (`series_ticker`)

Recurring market template identifier: `KXBTCD`, `KXBTC`, `KXETH`, etc.

---

## NATS Subject Routing

The connector routes messages to NATS subjects based on `type` and `market_ticker`:

```
{prefix}.json.{type}.{market_ticker}
```

Examples:
```
prod.kalshi.crypto.json.ticker.KXBTCD-26FEB14-T100000
prod.kalshi.crypto.json.trade.KXBTCD-26FEB14-T100000
prod.kalshi.crypto.json.market_lifecycle_v2.KXBTCD-26FEB14-T100000
prod.kalshi.crypto.json.event_lifecycle.KXBTCD-26JAN2310
```

For lifecycle messages, the ticker comes from `msg.market_ticker` (market lifecycle)
or `msg.event_ticker` (event lifecycle).

---

## Connector Message Flow

```
WS recv_raw()
    │
    ├── Parse JSON → WsMessage enum
    │
    ├── Match variant:
    │   ├── Ticker        → forward raw JSON bytes to mpsc channel
    │   ├── Trade         → forward raw JSON bytes
    │   ├── OrderbookSnapshot/Delta → forward raw JSON bytes
    │   ├── MarketLifecycleV2 → forward raw JSON bytes
    │   ├── EventLifecycle → forward raw JSON bytes
    │   ├── Subscribed/Ok → log, do NOT forward
    │   ├── Error         → log + warn, do NOT forward
    │   ├── Unsubscribed  → log, do NOT forward
    │   └── Unknown       → warn + log raw text, do NOT forward
    │
    └── mpsc channel → NatsWriter → NATS publish (fire-and-forget)
```

**Key**: The connector passes through the **original raw JSON bytes** from the
WebSocket, not a re-serialized version. This preserves all fields including
those not captured by the Rust structs.

---

## Archiver Validation Rules

The archiver performs lightweight validation on a sample (1-in-100) of messages:

### Ticker Validation
Required fields: `msg.market_ticker` (string), `msg.ts` (integer)

### Trade Validation
Required fields: `msg.market_ticker` (string), `msg.trade_id` (string),
`msg.yes_price` or `msg.price` (integer), `msg.count` (integer),
`msg.taker_side` or `msg.side` (string), `msg.ts` (integer)

### Lifecycle Validation
Required fields: `msg.market_ticker` (string), `msg.event_type` (string)

---

## WebSocket Connection Details

| Parameter | Value |
|-----------|-------|
| Production URL | `wss://api.kalshi.com/trade-api/ws/v2` |
| Demo URL | `wss://demo-api.kalshi.co/trade-api/ws/v2` |
| Auth headers | `KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-SIGNATURE`, `KALSHI-ACCESS-TIMESTAMP` |
| Max markets per subscription | 256 |
| Read timeout | 120 seconds (connector exits on timeout) |
| Ping interval | 30 seconds (connector sends WS pings) |
| Subscription timeout | 30 seconds (waiting for ack) |
| Reconnection | Exit process, K8s restarts pod |

---

## Parquet Schema Summary

For reference, the fields extracted from raw JSON into typed parquet columns:

### kalshi_ticker (v1.1.0)

| Column | Arrow Type | Nullable | Source JSON Path |
|--------|-----------|----------|------------------|
| `market_ticker` | Utf8 | no | `msg.market_ticker` |
| `yes_bid` | Int64 | yes | `msg.yes_bid` |
| `yes_ask` | Int64 | yes | `msg.yes_ask` |
| `no_bid` | Int64 | yes | `msg.no_bid` |
| `no_ask` | Int64 | yes | `msg.no_ask` |
| `last_price` | Int64 | yes | `msg.price` |
| `volume` | Int64 | yes | `msg.volume` |
| `open_interest` | Int64 | yes | `msg.open_interest` |
| `ts` | Timestamp(us, UTC) | no | `msg.ts` * 1,000,000 |
| `exchange_clock` | Int64 | yes | `msg.Clock` |
| `_nats_seq` | UInt64 | no | NATS metadata |
| `_received_at` | Timestamp(us, UTC) | no | Archiver clock |

### kalshi_trade (v1.1.0)

| Column | Arrow Type | Nullable | Source JSON Path |
|--------|-----------|----------|------------------|
| `market_ticker` | Utf8 | no | `msg.market_ticker` |
| `price` | Int64 | no | `msg.yes_price` or `msg.price` |
| `count` | Int64 | no | `msg.count` |
| `side` | Utf8 | no | `msg.taker_side` or `msg.side` |
| `ts` | Timestamp(us, UTC) | no | `msg.ts` * 1,000,000 |
| `trade_id` | Utf8 | no | `msg.trade_id` |
| `exchange_seq` | Int64 | yes | envelope `seq` |
| `_nats_seq` | UInt64 | no | NATS metadata |
| `_received_at` | Timestamp(us, UTC) | no | Archiver clock |

### kalshi_lifecycle (v1.0.0)

| Column | Arrow Type | Nullable | Source JSON Path |
|--------|-----------|----------|------------------|
| `market_ticker` | Utf8 | no | `msg.market_ticker` |
| `event_type` | Utf8 | no | `msg.event_type` |
| `open_ts` | Timestamp(us, UTC) | yes | `msg.open_ts` * 1,000,000 |
| `close_ts` | Timestamp(us, UTC) | yes | `msg.close_ts` * 1,000,000 |
| `additional_metadata` | Utf8 (JSON string) | yes | `msg.additional_metadata` (serialized) |
| `_nats_seq` | UInt64 | no | NATS metadata |
| `_received_at` | Timestamp(us, UTC) | no | Archiver clock |
