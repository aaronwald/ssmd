# 1m OHLCV Bar Schema (Derived Product)

**Last Updated**: 2026-06-29
**Served by**: `GET /v1/data/ohlcv/1m`
**Source**: `ssmd-rust/crates/ssmd-bar-cache/src/agg.rs` (`Bar` struct)

> **This is a DERIVED product, not a raw archived feed message.** These 1-minute bars are
> aggregated live by `ssmd-bar-cache` from the raw trade feeds and served verbatim from
> Redis. They are **not** parquet, **not** a raw archived message type, and **not** listed
> in the `/v1/data/schema-versions` registry (which covers only raw archived feed schemas).
> For the raw archived trade schemas the bars are built from, see
> [parquet-schemas.md](parquet-schemas.md) and the per-exchange JSON docs.

---

## Overview

`ssmd-bar-cache` consumes raw trade messages from NATS, rolls them into 1-minute OHLCV
bars with trade-count and aggressor attribution, and caches a rolling window per symbol in
Redis. The `GET /v1/data/ohlcv/1m` endpoint serves those bars directly.

Supported feeds (`BAR_CACHE_FEEDS`): `massive`, `kraken-spot`, `binance`.

---

## Response Envelope

```json
{
  "feed": "binance",
  "sym": "BTCUSDT",
  "bars": [ /* Bar objects, oldest -> newest */ ],
  "served_at": 1718658060000
}
```

| Field | JSON Type | Description |
|-------|-----------|-------------|
| `feed` | string | One of `massive`, `kraken-spot`, `binance` |
| `sym` | string | Symbol requested (the `sym` query param) |
| `bars` | array | Array of `Bar` objects, ordered **oldest to newest** |
| `served_at` | integer | Server response time, epoch ms |

**Query params**: `feed`, `sym` (e.g. `BTCUSDT`; kraken-spot uses pairs like `BTC/USDT`,
URL-encode the slash as `%2F`), and `limit`.

---

## Bar Fields

Serde JSON, snake_case:

| Field | JSON Type | Notes |
|-------|-----------|-------|
| `sym` | string | Symbol |
| `o` | number | Open |
| `h` | number | High |
| `l` | number | Low |
| `c` | number | Close |
| `v` | number | Total base-asset volume in the minute |
| `trade_count` | integer | Number of trades in the minute (0 for the massive 1s-bar source) |
| `taker_buy_volume` | number | Aggressor (taker) volume where the buyer was the taker |
| `taker_sell_volume` | number | Aggressor volume where the seller was the taker |
| `market_order_volume` | number | Volume from market orders; kraken-spot only (from `ord_type`); 0 for binance (no order-type data) and massive |
| `start_ts_ms` | integer | Minute start, epoch ms (inclusive) |
| `end_ts_ms` | integer | Minute end, epoch ms (exclusive; `start_ts_ms + 60_000`) |

### Example bar

```json
{
  "sym": "BTCUSDT",
  "o": 67000.5,
  "h": 67010.0,
  "l": 66990.0,
  "c": 67005.0,
  "v": 12.5,
  "trade_count": 340,
  "taker_buy_volume": 7.0,
  "taker_sell_volume": 5.5,
  "market_order_volume": 0,
  "start_ts_ms": 1718658000000,
  "end_ts_ms": 1718658060000
}
```

---

## Feed / Field Caveats

- **binance**: `taker_buy_volume + taker_sell_volume ≈ v` (every trade has a taker, split
  by side). `market_order_volume` is 0 (Binance trades carry no order-type data).
- **kraken-spot**: `market_order_volume` is populated from the trade `ord_type` field. An
  absent/unknown trade `side` yields 0 on **both** taker sides — no fabricated attribution.
- **massive**: bars are derived from 1s-bar source data, which carries no trade-level
  detail. All trade-derived fields (`trade_count`, `taker_buy_volume`, `taker_sell_volume`,
  `market_order_volume`) are 0.

---

**Source:** `ssmd-rust/crates/ssmd-bar-cache/src/agg.rs`
