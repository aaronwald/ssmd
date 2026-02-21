# Kraken Futures WebSocket JSON Schemas

**Last Updated**: 2026-02-15
**API Version**: WebSocket v1 (`wss://futures.kraken.com/ws/v1`)
**Connector Version**: ssmd-connector v0.8.9+ (Rust)
**Archiver Format**: Raw WebSocket JSON (no envelope wrapper)

### Exchange Properties

| Property | Value |
|----------|-------|
| **Sequenced** | Yes — `seq` on trade messages (per-product_id, monotonically increasing) |
| **Gap detection** | Trade only, via `seq` column in parquet (per-product_id GROUP BY). Ticker has no sequence. |
| **Identifier** | `product_id` — prefix-coded string (e.g. `PF_XBTUSD`, `PI_ETHUSD`) |
| **Timestamps** | Epoch milliseconds (UTC) |
| **Price units** | Decimal (e.g. `97000.5`) |

---

## Overview

Kraken Futures (formerly Crypto Facilities) uses a **WebSocket v1** protocol that is
distinct from the Kraken Spot v2 WebSocket. Our ssmd connector subscribes to the
**trade** and **ticker** feeds and publishes raw JSON to NATS.

### V1 (Futures) vs V2 (Spot) Format Distinction

This is a critical distinction. Our Kraken Futures connector receives **V1 format only**.

| Aspect | V1 (Futures) — what we use | V2 (Spot) — NOT used |
|--------|----------------------------|----------------------|
| Endpoint | `wss://futures.kraken.com/ws/v1` | `wss://ws.kraken.com/v2` |
| Data structure | **Flat object** with `feed` + `product_id` at top level | Nested `channel` + `data[]` array |
| Discriminator field | `feed` (e.g. `"ticker"`, `"trade"`) | `channel` (e.g. `"ticker"`, `"trade"`) |
| Identifier field | `product_id` (e.g. `"PF_XBTUSD"`) | `symbol` (e.g. `"BTC/USD"`) |
| Control messages | `event` field (e.g. `"subscribed"`, `"heartbeat"`) | `method`/`result` fields |
| Subscribe format | `{"event":"subscribe","feed":"...","product_ids":[...]}` | `{"method":"subscribe","params":{"channel":"...","symbol":[...]}}` |

**Why V1?** Kraken operates two separate WebSocket APIs. The Futures platform
(derivatives, perpetuals, fixed futures) exclusively uses V1. The Spot platform uses V2.
They are completely different protocols served from different hostnames. Our connector
only handles Futures products.

### Field Name Inconsistency

Kraken Futures sends **mixed camelCase and snake_case** within the same message. This is
a known API inconsistency, not a bug in our connector:

- **snake_case**: `product_id`, `bid_size`, `ask_size`, `funding_rate`, `funding_rate_prediction`
- **camelCase**: `markPrice`, `openInterest`, `volumeQuote`
- **lowercase**: `bid`, `ask`, `last`, `volume`, `index`, `time`, `change`, `premium`

All downstream consumers (funding-rate-consumer, ssmd-schemas) handle both conventions
defensively using fallback lookups (e.g. `msg.markPrice ?? msg.mark_price`).

### Subscribed Feeds

The connector subscribes to these feeds (see `connector.rs:161`):

| Feed | NATS Subject Pattern | Archived? | Parquet Schema? |
|------|---------------------|-----------|-----------------|
| `ticker` | `prod.kraken-futures.json.ticker.{product_id}` | Yes | `kraken_futures_ticker` v1.0.0 |
| `trade` | `prod.kraken-futures.json.trade.{product_id}` | Yes | `kraken_futures_trade` v1.0.0 |

Feeds **not** subscribed: `book`, `book_snapshot`, `ticker_lite`, `trade_snapshot`.

### Archive Format

Raw WebSocket JSON lines (no envelope wrapper), unlike Kalshi which uses
`{"type":"ticker","sid":1,"msg":{...}}` envelope format:

```
{"feed":"ticker","product_id":"PF_XBTUSD","bid":97000.0,...}
{"feed":"trade","product_id":"PI_XBTUSD","side":"buy",...}
```

---

## Data Messages

### ticker

**Exchange docs**: [Ticker](https://docs.kraken.com/api/docs/futures-api/websocket/ticker)

The primary data feed. Provides a full snapshot of the current market state for a
product, updated on every change (price, volume, funding rate, etc.).

**Frequency**: High — multiple updates per second during active trading.
**Importance**: Contains funding rate fields critical for Phase 2 signal generation.

#### Example

```json
{
  "feed": "ticker",
  "product_id": "PF_ETHUSD",
  "bid": 1907.3,
  "bid_size": 0.097,
  "ask": 1907.4,
  "ask_size": 0.112,
  "volume": 56514.738,
  "dtm": 0,
  "leverage": "50x",
  "index": 1907.83,
  "last": 1907.7,
  "time": 1770920339237,
  "change": -1.45,
  "premium": 0.1,
  "funding_rate": -0.005,
  "funding_rate_prediction": -0.014,
  "next_funding_rate_time": 1770922800000,
  "markPrice": 1907.459,
  "openInterest": 17059.147,
  "volumeQuote": 110391688.155,
  "open": 1935.7,
  "high": 2000.2,
  "low": 1895.0,
  "suspended": false,
  "tag": "perpetual",
  "pair": "ETH:USD",
  "post_only": false,
  "fundingRatePrediction": -0.014
}
```

#### Field Reference

| Field | JSON Type | Required | Description | Units/Format |
|-------|-----------|----------|-------------|-------------|
| `feed` | string | Yes | Always `"ticker"` | Discriminator |
| `product_id` | string | Yes | Product symbol | `PF_XBTUSD`, `PF_ETHUSD`, `PI_XBTUSD`, `FF_XBTUSD_260227` |
| `bid` | number | Yes | Best bid price | USD (or quote currency) |
| `bid_size` | number | Yes | Size at best bid | Contracts (base currency units) |
| `ask` | number | Yes | Best ask price | USD |
| `ask_size` | number | Yes | Size at best ask | Contracts |
| `last` | number | Yes | Last traded price | USD |
| `volume` | number | Yes | 24-hour trading volume | Base currency units |
| `time` | number | Yes | Message timestamp | **Epoch milliseconds** (UTC) |
| `dtm` | number | No | Days to maturity | Integer; 0 for perpetuals |
| `leverage` | string | No | Maximum leverage | e.g. `"50x"`, `"2x"` |
| `index` | number | No | Underlying index price | USD |
| `change` | number | No | 24-hour price change | USD (absolute) |
| `premium` | number | No | Premium over index | Percentage |
| `funding_rate` | number | No | Current hourly funding rate | Fraction (e.g. 0.0001 = 0.01%/hr) |
| `funding_rate_prediction` | number | No | Predicted next funding rate | Fraction |
| `next_funding_rate_time` | number | No | Next funding rate application time | Epoch milliseconds (UTC) |
| `markPrice` | number | No | Mark price (for liquidation) | USD |
| `openInterest` | number | No | Total open interest | Base currency units (e.g. BTC) |
| `volumeQuote` | number | No | 24-hour volume in quote currency | USD |
| `open` | number | No | 24-hour opening price | USD |
| `high` | number | No | 24-hour high price | USD |
| `low` | number | No | 24-hour low price | USD |
| `suspended` | boolean | No | Whether trading is suspended | `true`/`false` |
| `tag` | string | No | Product type tag | `"perpetual"`, `"month"`, `"quarter"` |
| `pair` | string | No | Underlying pair | e.g. `"XBT:USD"`, `"ETH:USD"` |
| `post_only` | boolean | No | Post-only mode active | `true`/`false` |
| `fundingRatePrediction` | number | No | Duplicate of `funding_rate_prediction` (camelCase) | Fraction |

**Validation rules** (from `validation.rs`): Required fields for DQ validation are
`product_id`, `bid`, `ask`, `last`, `volume`, `time`.

**Parquet schema** maps these JSON fields to Arrow columns — see field mapping notes
in the Parquet Schema section below.

#### Funding Rate Fields (Phase 2 Critical)

| Field | Range | Notes |
|-------|-------|-------|
| `funding_rate` | Typically -0.0025 to +0.0025 | Hourly rate; **capped at +/-0.25%/hr** (no dampener) |
| `funding_rate_prediction` | Same range | Kraken's estimate of next hour's rate |
| `next_funding_rate_time` | Epoch ms | Next application time (hourly boundary) |

- **Positive rate**: longs pay shorts (bullish market bias)
- **Negative rate**: shorts pay longs (bearish market bias)
- **Zero rate**: Market in equilibrium — this is normal, not an error
- Kraken's no-dampener design makes funding rates **more volatile** than Binance/OKX
- Extreme funding (approaching +/-0.25% cap) is rare but represents the alpha opportunity
- The funding-rate-consumer flushes these to `pair_snapshots` table every 5 minutes

---

### trade

**Exchange docs**: [Trade](https://docs.kraken.com/api/docs/futures-api/websocket/trade)

Individual fill events. Each message represents one matched trade.

**Frequency**: Event-driven — one message per fill.

#### Example

```json
{
  "feed": "trade",
  "product_id": "PF_XBTUSD",
  "uid": "16dc1852-127b-40c9-acbb-5916ce98ee04",
  "side": "sell",
  "type": "fill",
  "seq": 96905,
  "time": 1770920339688,
  "qty": 0.0003,
  "price": 65368.0
}
```

#### Field Reference

| Field | JSON Type | Required | Description | Units/Format |
|-------|-----------|----------|-------------|-------------|
| `feed` | string | Yes | Always `"trade"` | Discriminator |
| `product_id` | string | Yes | Product symbol | e.g. `"PF_XBTUSD"` |
| `uid` | string | Yes | Unique trade identifier | UUID v4 (e.g. `"16dc1852-127b-40c9-acbb-5916ce98ee04"`) |
| `side` | string | Yes | Taker side | `"buy"` or `"sell"` |
| `type` | string | Yes | Trade type | `"fill"` (may have other values) |
| `seq` | number | Yes | Sequence number | Integer, monotonically increasing |
| `time` | number | Yes | Trade timestamp | **Epoch milliseconds** (UTC) |
| `qty` | number | Yes | Trade quantity | Base currency units (e.g. BTC) |
| `price` | number | Yes | Trade price | USD |

**Validation rules** (from `validation.rs`): Required fields for DQ validation are
`product_id`, `uid`, `side`, `price`, `qty`, `time`.

#### Sequence Number (`seq`) Semantics

- **Scope**: Per-product. Each `product_id` has its own monotonically increasing sequence.
- **Monotonicity**: Strictly increasing within a product — gaps indicate missed trades.
- **Not global**: A `seq` of 100 on `PF_XBTUSD` and `seq` of 100 on `PF_ETHUSD` are
  unrelated.
- **Persistence**: Sequences survive Kraken server restarts (they are exchange-assigned,
  not session-scoped).

**Important**: This `seq` is the **exchange-assigned** trade sequence. It is distinct
from the NATS JetStream `stream_sequence` which is assigned by JetStream and tracked
in the archiver manifest. See `data-pipeline.md` for the two sequence domains.

#### Trade UID

The `uid` field is a UUID v4 assigned by Kraken to uniquely identify each trade globally.
It can be used for deduplication across reconnections or archive replays. Our DQ system
currently uses NATS `_nats_seq` for duplicate detection rather than trade `uid`.

---

### trade_snapshot

**Exchange docs**: [Trade](https://docs.kraken.com/api/docs/futures-api/websocket/trade) (snapshot section)

Sent immediately after subscribing to the `trade` feed. Contains recent trade history.
Our connector recognizes this feed type in metrics (`connector.rs:103`) and counts it
as a trade message.

**Note**: Our connector does **not** explicitly subscribe to `trade_snapshot` — it is
sent automatically by Kraken as part of the `trade` subscription as the initial snapshot
before real-time updates begin.

#### Example

```json
{
  "feed": "trade_snapshot",
  "product_id": "PF_XBTUSD",
  "trades": [
    {
      "uid": "abc-123",
      "side": "buy",
      "type": "fill",
      "seq": 96900,
      "time": 1770920300000,
      "qty": 0.01,
      "price": 65350.0
    },
    {
      "uid": "def-456",
      "side": "sell",
      "type": "fill",
      "seq": 96901,
      "time": 1770920310000,
      "qty": 0.005,
      "price": 65355.0
    }
  ]
}
```

#### Field Reference

| Field | JSON Type | Required | Description |
|-------|-----------|----------|-------------|
| `feed` | string | Yes | Always `"trade_snapshot"` |
| `product_id` | string | Yes | Product symbol |
| `trades` | array | Yes | Array of recent trade objects (same fields as individual `trade` messages) |

**NATS routing**: The writer routes `trade_snapshot` messages to the same NATS subject
pattern as `trade` messages (`writer.rs:82`).

**Parquet handling**: No dedicated parquet schema exists for `trade_snapshot`. These
messages are not parsed into the `kraken_futures_trade` schema (which expects individual
flat trade objects, not a `trades` array wrapper). They appear in the JSONL archive
but would be skipped during parquet generation.

---

### ticker_lite

**Exchange docs**: [Ticker Lite](https://docs.kraken.com/api/docs/futures-api/websocket/ticker-lite)

A lightweight variant of the ticker feed with fewer fields. Tracked in connector
metrics (`connector.rs:102`) but **not actively subscribed** in our deployment.

#### Expected Structure

```json
{
  "feed": "ticker_lite",
  "product_id": "PF_XBTUSD",
  "bid": 97000.0,
  "ask": 97001.0,
  "last": 97000.0,
  "change": 50.0,
  "time": 1707300000000
}
```

**NATS routing**: The writer does **not** route `ticker_lite` messages — they fall
through to the "Unknown Kraken Futures feed, skipping" debug log (`writer.rs:84`).

---

### book_snapshot / book

**Exchange docs**: [Book](https://docs.kraken.com/api/docs/futures-api/websocket/book)

Orderbook data. The `book_snapshot` feed provides the initial L2 orderbook state;
`book` provides incremental updates. Tracked in connector metrics (`connector.rs:104`)
but **not actively subscribed** in our deployment.

#### Expected book_snapshot Structure

```json
{
  "feed": "book_snapshot",
  "product_id": "PF_XBTUSD",
  "timestamp": 1707300000000,
  "seq": 12345,
  "bids": [
    { "price": 97000.0, "qty": 1.5 },
    { "price": 96999.0, "qty": 2.0 }
  ],
  "asks": [
    { "price": 97001.0, "qty": 0.8 },
    { "price": 97002.0, "qty": 1.2 }
  ]
}
```

#### Expected book (delta) Structure

```json
{
  "feed": "book",
  "product_id": "PF_XBTUSD",
  "timestamp": 1707300001000,
  "seq": 12346,
  "side": "buy",
  "price": 97000.5,
  "qty": 0.5
}
```

**Note**: Since we don't subscribe to book feeds, these structures are based on the
standard Kraken Futures API documentation and have not been verified against live data
in our pipeline. `qty` of 0 in a delta indicates a price level removal.

---

## Control Messages

Control messages use an `event` field as discriminator. They are **not** published to
NATS — the connector filters them out. They do not appear in archives.

### info

Sent once immediately upon WebSocket connection.

```json
{
  "event": "info",
  "version": 1
}
```

| Field | JSON Type | Description |
|-------|-----------|-------------|
| `event` | string | Always `"info"` |
| `version` | number | API version (always `1` for Futures v1) |

### subscribed

Acknowledgment after a successful subscription request.

```json
{
  "event": "subscribed",
  "feed": "ticker",
  "product_ids": ["PF_XBTUSD", "PF_ETHUSD"]
}
```

| Field | JSON Type | Description |
|-------|-----------|-------------|
| `event` | string | Always `"subscribed"` |
| `feed` | string | The feed that was subscribed |
| `product_ids` | string[] | The products successfully subscribed |

### heartbeat

Periodic keep-alive from the server.

```json
{
  "event": "heartbeat"
}
```

| Field | JSON Type | Description |
|-------|-----------|-------------|
| `event` | string | Always `"heartbeat"` |

The connector logs heartbeats at `trace` level and does not forward them to NATS.
The connector also sends WebSocket-level ping frames every 30 seconds
(`PING_INTERVAL_SECS = 30`) to keep the connection alive.

### error

Server-side error notification.

```json
{
  "event": "error",
  "message": "Unknown product: INVALID_PRODUCT"
}
```

| Field | JSON Type | Description |
|-------|-----------|-------------|
| `event` | string | Always `"error"` |
| `message` | string | Human-readable error description |

Errors during subscription cause the connector to fail with `ServerError`. Errors
during normal operation are logged as warnings.

### challenge

Sent for authenticated (private) feeds. Not used in our deployment since we only
subscribe to public feeds (`auth_method: none` in `kraken-futures.yaml`).

```json
{
  "event": "challenge",
  "message": "a-uuid-challenge-string"
}
```

---

## Subscription Protocol

### Subscribe Request

```json
{
  "event": "subscribe",
  "feed": "ticker",
  "product_ids": ["PF_XBTUSD", "PF_ETHUSD"]
}
```

Our connector subscribes to `trade` and `ticker` feeds sequentially (`connector.rs:161`),
waiting for a `subscribed` acknowledgment for each before proceeding. Subscription
timeout is 30 seconds (`SUBSCRIBE_TIMEOUT_SECS`).

### Unsubscribe Request

```json
{
  "event": "unsubscribe",
  "feed": "ticker",
  "product_ids": ["PF_XBTUSD"]
}
```

Not used by our connector — we subscribe on connect and never unsubscribe.

---

## Symbol / Identifier Format

### product_id Prefixes

| Prefix | Type | Settlement | Example |
|--------|------|-----------|---------|
| `PF_` | Perpetual Futures (linear) | Never (funding rate) | `PF_XBTUSD`, `PF_ETHUSD` |
| `PI_` | Perpetual Inverse | Never (inverse contract) | `PI_XBTUSD` |
| `FF_` | Fixed Futures (linear) | Monthly/quarterly expiry | `FF_XBTUSD_260227` |
| `FI_` | Fixed Inverse | Monthly/quarterly expiry | `FI_XBTUSD_260227` |

**Expiry suffix**: Fixed futures include a date suffix in `YYMMDD` format
(e.g. `_260227` = Feb 27, 2026).

### Mapping to Secmaster

In the pair_snapshots and pairs tables, Kraken Futures products are identified as:

```
kraken:{product_id}
```

For example: `kraken:PF_XBTUSD`, `kraken:PF_ETHUSD`

This namespacing was introduced in migration 0012 to avoid collisions with Kraken Spot pairs.

### NATS Subject Routing

```
prod.kraken-futures.json.{feed_type}.{product_id}
```

Examples:
- `prod.kraken-futures.json.ticker.PF_XBTUSD`
- `prod.kraken-futures.json.trade.PI_XBTUSD`
- `prod.kraken-futures.json.trade.PF_ETHUSD`

Product IDs pass through `sanitize_subject_token()` unchanged — characters like
underscores are NATS-safe (`writer.rs:119`).

---

## Timestamp Format

All timestamps in Kraken Futures WebSocket messages are **epoch milliseconds** (UTC).

| Field | Format | Example | Notes |
|-------|--------|---------|-------|
| `time` (ticker) | Epoch ms | `1770920339237` | Message generation time |
| `time` (trade) | Epoch ms | `1770920339688` | Trade execution time |
| `next_funding_rate_time` | Epoch ms | `1770922800000` | Next funding application time (hourly boundary) |

**Contrast with Kalshi**: Kalshi uses **epoch seconds** for its `ts` field.
Parsers must handle this difference — the ssmd-schemas `epoch_ms_to_micros()` function
converts Kraken's milliseconds to Arrow Timestamp(Microsecond) for parquet output.

**pair_snapshots `snapshot_at`**: Uses PostgreSQL `TIMESTAMPTZ` from the server clock
at flush time (not the Kraken message `time`). There is ~seconds of jitter between
the Kraken message time and the recorded snapshot time, which is negligible at the
5-minute flush resolution.

---

## Parquet Schema Mapping

### kraken_futures_ticker (v1.0.0)

21 columns. JSON field names map to Arrow columns with some renaming:

| # | Arrow Column | Arrow Type | JSON Source Field | Nullable | Notes |
|---|-------------|-----------|-------------------|----------|-------|
| 0 | `product_id` | Utf8 | `product_id` | No | |
| 1 | `bid` | Float64 | `bid` | No | |
| 2 | `bid_size` | Float64 | `bid_size` | No | |
| 3 | `ask` | Float64 | `ask` | No | |
| 4 | `ask_size` | Float64 | `ask_size` | No | |
| 5 | `last` | Float64 | `last` | No | |
| 6 | `volume` | Float64 | `volume` | No | |
| 7 | `volume_quote` | Float64 | `volumeQuote` | Yes | **camelCase source** |
| 8 | `open` | Float64 | `open` | Yes | |
| 9 | `high` | Float64 | `high` | Yes | |
| 10 | `low` | Float64 | `low` | Yes | |
| 11 | `change` | Float64 | `change` | Yes | |
| 12 | `index_price` | Float64 | `index` | Yes | **renamed**: `index` -> `index_price` |
| 13 | `mark_price` | Float64 | `markPrice` | Yes | **camelCase source** |
| 14 | `open_interest` | Float64 | `openInterest` | Yes | **camelCase source** |
| 15 | `funding_rate` | Float64 | `funding_rate` | Yes | |
| 16 | `funding_rate_prediction` | Float64 | `funding_rate_prediction` | Yes | |
| 17 | `next_funding_rate_time` | Int64 | `next_funding_rate_time` | Yes | Raw epoch ms (not converted to Timestamp) |
| 18 | `time` | Timestamp(us, UTC) | `time` | No | Converted: epoch ms * 1000 -> microseconds |
| 19 | `_nats_seq` | UInt64 | (NATS metadata) | No | JetStream stream sequence |
| 20 | `_received_at` | Timestamp(us, UTC) | (archiver clock) | No | When archiver received the message |

### kraken_futures_trade (v1.0.0)

10 columns:

| # | Arrow Column | Arrow Type | JSON Source Field | Nullable | Notes |
|---|-------------|-----------|-------------------|----------|-------|
| 0 | `product_id` | Utf8 | `product_id` | No | |
| 1 | `uid` | Utf8 | `uid` | No | UUID v4 trade ID |
| 2 | `side` | Utf8 | `side` | No | `"buy"` or `"sell"` |
| 3 | `trade_type` | Utf8 | `type` | No | **renamed**: `type` -> `trade_type` (reserved word) |
| 4 | `seq` | Int64 | `seq` | No | Exchange trade sequence (per-product) |
| 5 | `qty` | Float64 | `qty` | No | |
| 6 | `price` | Float64 | `price` | No | |
| 7 | `time` | Timestamp(us, UTC) | `time` | No | Converted: epoch ms * 1000 -> microseconds |
| 8 | `_nats_seq` | UInt64 | (NATS metadata) | No | JetStream stream sequence |
| 9 | `_received_at` | Timestamp(us, UTC) | (archiver clock) | No | When archiver received the message |

---

## Message Type Detection

The ssmd-schemas `detect_message_type()` function (`lib.rs:125`) determines message
type for Kraken Futures:

1. If the JSON has an `event` field -> **skip** (control message, no schema)
2. Otherwise, read the `feed` field -> return as message type string

This means:
- `{"feed":"ticker",...}` -> type = `"ticker"` -> `KrakenFuturesTickerSchema`
- `{"feed":"trade",...}` -> type = `"trade"` -> `KrakenFuturesTradeSchema`
- `{"feed":"ticker_lite",...}` -> type = `"ticker_lite"` -> no schema (skipped in parquet-gen)
- `{"feed":"trade_snapshot",...}` -> type = `"trade_snapshot"` -> no schema (skipped)
- `{"event":"subscribed",...}` -> skipped (control message)

---

## Current Deployment

| Property | Value |
|----------|-------|
| Connector | ssmd-connector (Rust), 1 shard |
| NATS Stream | `PROD_KRAKEN_FUTURES` |
| Stream Limits | 256 MB max, 48h retention, 2min dedup window |
| Archiver | `archiver-kraken-futures` writing to GCS |
| Archive Start | 2026-02-08 |
| Data Volume | ~10 MiB/day compressed |
| Subscribed Products | All available futures (PF_, PI_, FF_, FI_ prefixes) |
| Connector Module | `ssmd-rust/crates/connector/src/kraken_futures/` |

---

## Common Issues

| Issue | Cause | Detection | Fix |
|-------|-------|-----------|-----|
| Mixed field names in same message | Kraken API inconsistency | N/A (expected) | Handle both camelCase and snake_case defensively |
| Ticker not updating for a product | Product delisted or suspended | Check `suspended` field in ticker | Normal — product may be halted |
| `funding_rate` = 0 | Market in equilibrium | N/A | Normal — not an error |
| Large `openInterest` values | Units are in base currency (BTC) | N/A | Multiply by price for USD notional |
| `trade_snapshot` not in parquet | No dedicated parquet schema | DQ message type counts | By design — snapshot is initial history only |
| `ticker_lite` dropped by writer | Writer only routes `ticker` and `trade`/`trade_snapshot` | Debug logs | By design — subscribe to `ticker` (full) instead |
| Timestamp confusion with Kalshi | Kraken uses **ms**, Kalshi uses **s** | Timestamps 1000x too large/small | Use feed-specific conversion |
