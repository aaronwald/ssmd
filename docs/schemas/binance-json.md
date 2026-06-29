# Binance Spot WebSocket JSON Schemas

**Last Updated**: 2026-06-29
**API Version**: Spot WebSocket combined stream (`wss://data-stream.binance.vision/stream`)
**Connector Version**: ssmd-connector (Rust)
**Archiver Format**: Raw combined-stream JSON frame (nested `data` envelope)

### Exchange Properties

| Property | Value |
|----------|-------|
| **Sequenced** | No exchange sequence on `@trade`; gap detection relies on NATS `_nats_seq` |
| **Identifier** | `s` — symbol string (e.g. `BTCUSDT`, `ETHUSDT`), uppercased |
| **Timestamps** | Epoch milliseconds (UTC) |
| **Price units** | Decimal **strings** (e.g. `"67000.50"`) — not JSON numbers |

---

## Overview

The Binance Spot connector subscribes to the **combined stream** endpoint and publishes
each `@trade` frame to NATS verbatim. Unlike Kalshi (which wraps payloads in a
`{"type":...,"sid":...,"msg":{...}}` envelope) or Kraken Futures (flat top-level object),
Binance combined-stream frames carry the payload under a **nested `data` object**:

```json
{
  "stream": "btcusdt@trade",
  "data": {
    "e": "trade",
    "E": 1718658000123,
    "s": "BTCUSDT",
    "t": 88123456,
    "p": "67000.50",
    "q": "0.0125",
    "T": 1718658000123,
    "m": false,
    "M": true
  }
}
```

The trade is read from the inner `data` object. The connector consumes **only** `trade`
events — kline frames (`{"stream":"btcusdt@kline_1m",...}`) and control frames (no `data`,
e.g. a subscribe response) are skipped.

### Subscribed Feeds

| Feed | Stream Name | Archived? | Parquet Schema? |
|------|-------------|-----------|-----------------|
| `trade` | `{symbol}@trade` | Yes | `binance_trade` v1.0.0 |

Only `trade` is consumed. Klines, tickers, and depth streams are **not** subscribed and
are skipped if present.

### Archive Format

Raw combined-stream JSON frames (nested `data` envelope), one per line:

```
{"stream":"btcusdt@trade","data":{"e":"trade","E":...,"s":"BTCUSDT","t":...,"p":"...","q":"...","T":...,"m":false,"M":true}}
```

---

## Data Messages

### trade

**Exchange docs**: [Trade Streams](https://developers.binance.com/docs/binance-spot-api-docs/web-socket-streams#trade-streams)

Individual fill events. Each `@trade` frame represents one matched trade.

**Frequency**: Event-driven — one frame per trade.

#### Example

```json
{
  "stream": "btcusdt@trade",
  "data": {
    "e": "trade",
    "E": 1718658000123,
    "s": "BTCUSDT",
    "t": 88123456,
    "p": "67000.50",
    "q": "0.0125",
    "T": 1718658000123,
    "m": false,
    "M": true
  }
}
```

#### Field Reference

| Field | JSON Type | Required | Description | Units/Format |
|-------|-----------|----------|-------------|-------------|
| `stream` | string | Yes | Combined-stream routing key | e.g. `"btcusdt@trade"` (envelope, not archived) |
| `data` | object | Yes | Nested payload object | Trade lives here |
| `data.e` | string | Yes | Inner event type / discriminator | Always `"trade"` for trades |
| `data.E` | number | Yes | Event time | **Epoch milliseconds** (UTC) |
| `data.s` | string | Yes | Symbol | e.g. `"BTCUSDT"` (uppercased) |
| `data.t` | number | No | Trade ID | Integer — optional metadata |
| `data.p` | string | Yes | Trade price | **Decimal string** (e.g. `"67000.50"`) |
| `data.q` | string | Yes | Trade quantity | **Decimal string** (base-asset units) |
| `data.T` | number | Yes | Trade time | **Epoch milliseconds** (UTC) |
| `data.m` | boolean | No | Buyer is the market maker | `true`/`false` |
| `data.M` | boolean | No | Ignore (best-price-match flag) | `true`/`false` |

**String-typed numerics**: `data.p` (price) and `data.q` (qty) arrive as decimal
**strings**, not JSON numbers. They are parsed to `f64` on ingest; an unparseable string
skips the row (not coerced to zero).

**Timestamps**: `data.T` (trade time) and `data.E` (event time) are epoch **milliseconds**.
The parquet `exchange_ts_ms` column is sourced from `data.T`.

**Trade ID**: `data.t` is an integer. It is optional metadata — a trade missing only `t`
is still archived (with a null `trade_id`), never dropped (Complete Data Archive pillar).

**Maker flags**: `data.m` is the buyer-is-maker flag (used by the live 1m-bar aggregator
for taker-side attribution). `data.M` is a "best price match" flag that should be ignored.
Neither flag is materialized in the parquet schema.

---

## Message Type Detection

Detection keys off the **inner** `data.e` value, not the top-level frame:

1. If the frame has no `data` object -> **skip** (control frame, e.g. subscribe response)
2. Otherwise, read `data.e`:
   - `"trade"` -> `binance_trade` schema
   - any other value (e.g. `"kline"`) -> detected but not registered -> skipped in parquet-gen

This means:
- `{"stream":"btcusdt@trade","data":{"e":"trade",...}}` -> type = `"trade"` -> `BinanceTradeSchema`
- `{"stream":"btcusdt@kline_1m","data":{"e":"kline",...}}` -> type = `"kline"` -> no schema (skipped)
- `{"result":null,"id":1}` -> no `data` -> skipped (control frame)

---

## Parquet Mapping

The archived `@trade` frame is converted to the `binance_trade` parquet schema. See
[parquet-schemas.md#binance_trade](parquet-schemas.md#binance_trade) for the full Arrow
column layout, including the renamed wire keys (`data.s`/`p`/`q`/`T` -> `symbol`/`price`/
`qty`/`exchange_ts_ms`, `data.t` -> `trade_id`) and the note that the `m`/`M` maker flags
are intentionally not materialized.

**Source:** `ssmd-rust/crates/ssmd-schemas/src/binance.rs`
