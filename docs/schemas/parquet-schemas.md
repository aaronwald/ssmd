# Parquet Schema Reference

> Complete specification of Arrow schemas used by `ssmd-parquet-gen` to convert archived JSONL.gz into Parquet files.

**Source crate:** `ssmd-rust/crates/ssmd-schemas/`
**Generator:** `ssmd-rust/crates/ssmd-parquet-gen/`

---

## Table of Contents

- [Pipeline Overview](#pipeline-overview)
- [Schema Registry & Message Detection](#schema-registry--message-detection)
- [Kalshi Schemas](#kalshi-schemas)
  - [kalshi_ticker](#kalshi_ticker)
  - [kalshi_trade](#kalshi_trade)
  - [kalshi_lifecycle](#kalshi_lifecycle)
- [Kraken Spot Schemas](#kraken-spot-schemas)
  - [kraken_ticker](#kraken_ticker)
  - [kraken_trade](#kraken_trade)
- [Kraken Futures Schemas](#kraken-futures-schemas)
  - [kraken_futures_ticker](#kraken_futures_ticker)
  - [kraken_futures_trade](#kraken_futures_trade)
- [Polymarket Schemas](#polymarket-schemas)
  - [polymarket_book](#polymarket_book)
  - [polymarket_trade](#polymarket_trade)
  - [polymarket_price_change](#polymarket_price_change)
  - [polymarket_best_bid_ask](#polymarket_best_bid_ask)
- [Cross-Cutting Details](#cross-cutting-details)
  - [Parquet File Metadata](#parquet-file-metadata)
  - [File Naming & GCS Layout](#file-naming--gcs-layout)
  - [Pipeline-Injected Columns](#pipeline-injected-columns)
  - [Schema Versioning](#schema-versioning)
  - [WriterProperties](#writerproperties)

---

## Pipeline Overview

```
GCS (JSONL.gz)  -->  ssmd-parquet-gen  -->  GCS (Parquet)
```

The archiver writes raw WebSocket messages as JSONL.gz to GCS. `ssmd-parquet-gen` reads those files, detects message types, converts JSON to typed Arrow RecordBatches, and writes Snappy-compressed Parquet with embedded metadata.

---

## Schema Registry & Message Detection

The `SchemaRegistry` maps each feed to its registered message types. Detection logic is feed-specific:

| Feed | Detection Field | Detection Logic | Registered Types |
|------|----------------|-----------------|------------------|
| `kalshi` | `type` | `json["type"]` string value | `ticker`, `trade`, `market_lifecycle_v2` |
| `kraken` | `channel` + `data` | `json["channel"]` if `json["data"]` exists (skips control messages) | `ticker`, `trade` |
| `kraken-futures` | `feed` | `json["feed"]` if no `json["event"]` (skips subscription messages) | `ticker`, `trade` |
| `polymarket` | `event_type` | `json["event_type"]` string value | `book`, `last_trade_price`, `price_change`, `best_bid_ask` |

Messages whose detected type has no registered schema are silently skipped. Messages that fail detection (e.g., heartbeats, subscription confirmations) are counted as `lines_skipped`.

**Source:** `ssmd-schemas/src/lib.rs:113-139` (`detect_message_type()`)

---

## Kalshi Schemas

All Kalshi messages use an envelope structure: `{"type": "...", "sid": N, "msg": {...}}`. Schema parsers read fields from the inner `msg` object unless noted otherwise.

### kalshi_ticker

**Identity:** `schema_name = "kalshi_ticker"`, `schema_version = "1.1.0"`, `message_type = "ticker"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `market_ticker` | Utf8 | no | `msg.market_ticker` | Required, row skipped if missing |
| 1 | `yes_bid` | Int64 | yes | `msg.yes_bid` | Cents (0-99) |
| 2 | `yes_ask` | Int64 | yes | `msg.yes_ask` | Cents (0-99) |
| 3 | `no_bid` | Int64 | yes | `msg.no_bid` | Cents (0-99) |
| 4 | `no_ask` | Int64 | yes | `msg.no_ask` | Cents (0-99) |
| 5 | `last_price` | Int64 | yes | `msg.price` | JSON field is `price`, column renamed to `last_price` |
| 6 | `volume` | Int64 | yes | `msg.volume` | |
| 7 | `open_interest` | Int64 | yes | `msg.open_interest` | |
| 8 | `ts` | Timestamp(us, UTC) | no | `msg.ts` | Unix seconds in JSON, converted: `ts * 1_000_000` |
| 9 | `exchange_clock` | Int64 | yes | `msg.Clock` | Note: capital `C` in JSON field name |
| 10 | `_nats_seq` | UInt64 | no | pipeline | Line counter (1-indexed) |
| 11 | `_received_at` | Timestamp(us, UTC) | no | pipeline | Hour boundary timestamp |

**JSON fields NOT in parquet:** `type` (envelope), `sid` (subscription ID)

**Source:** `ssmd-schemas/src/kalshi.rs:27-139`

---

### kalshi_trade

**Identity:** `schema_name = "kalshi_trade"`, `schema_version = "1.1.0"`, `message_type = "trade"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `market_ticker` | Utf8 | no | `msg.market_ticker` | Required |
| 1 | `price` | Int64 | no | `msg.yes_price` or `msg.price` | Tries `yes_price` first (raw WS name), falls back to `price` (connector alias) |
| 2 | `count` | Int64 | no | `msg.count` | Number of contracts |
| 3 | `side` | Utf8 | no | `msg.taker_side` or `msg.side` | Tries `taker_side` first (raw WS name), falls back to `side` |
| 4 | `ts` | Timestamp(us, UTC) | no | `msg.ts` | Unix seconds, converted: `ts * 1_000_000` |
| 5 | `trade_id` | Utf8 | no | `msg.trade_id` | UUID string, required |
| 6 | `exchange_seq` | Int64 | yes | `json.seq` | Top-level envelope field, NOT inside `msg` |
| 7 | `_nats_seq` | UInt64 | no | pipeline | Line counter |
| 8 | `_received_at` | Timestamp(us, UTC) | no | pipeline | Hour boundary timestamp |

**Field aliasing:** The Kalshi WebSocket sends `yes_price` and `taker_side`, while the connector may alias these to `price` and `side`. The parser accepts both forms, checking the raw WS name first.

**JSON fields NOT in parquet:** `type`, `sid`

**Source:** `ssmd-schemas/src/kalshi.rs:145-297`

---

### kalshi_lifecycle

**Identity:** `schema_name = "kalshi_lifecycle"`, `schema_version = "1.0.0"`, `message_type = "market_lifecycle_v2"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `market_ticker` | Utf8 | no | `msg.market_ticker` | Required |
| 1 | `event_type` | Utf8 | no | `msg.event_type` | e.g., `activated`, `created`, `settled` |
| 2 | `open_ts` | Timestamp(us, UTC) | yes | `msg.open_ts` | Unix seconds, converted to microseconds |
| 3 | `close_ts` | Timestamp(us, UTC) | yes | `msg.close_ts` | Unix seconds, converted to microseconds |
| 4 | `additional_metadata` | Utf8 | yes | `msg.additional_metadata` | JSON object serialized as string via `to_string()` |
| 5 | `_nats_seq` | UInt64 | no | pipeline | Line counter |
| 6 | `_received_at` | Timestamp(us, UTC) | no | pipeline | Hour boundary timestamp |

**JSON fields NOT in parquet:** `type`, `sid`

**Source:** `ssmd-schemas/src/kalshi.rs:303-407`

---

## Kraken Spot Schemas

Kraken Spot V2 API messages use: `{"channel": "...", "type": "update", "data": [...]}`. The `data` array may contain multiple items; each item produces a separate row. Messages without a `data` field (heartbeats, subscription results) are skipped.

### kraken_ticker

**Identity:** `schema_name = "kraken_ticker"`, `schema_version = "1.0.0"`, `message_type = "ticker"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `symbol` | Utf8 | no | `data[].symbol` | e.g., `BTC/USD` |
| 1 | `bid` | Float64 | no | `data[].bid` | |
| 2 | `bid_qty` | Float64 | no | `data[].bid_qty` | |
| 3 | `ask` | Float64 | no | `data[].ask` | |
| 4 | `ask_qty` | Float64 | no | `data[].ask_qty` | |
| 5 | `last` | Float64 | no | `data[].last` | |
| 6 | `volume` | Float64 | no | `data[].volume` | |
| 7 | `vwap` | Float64 | no | `data[].vwap` | |
| 8 | `high` | Float64 | no | `data[].high` | |
| 9 | `low` | Float64 | no | `data[].low` | |
| 10 | `change` | Float64 | no | `data[].change` | |
| 11 | `change_pct` | Float64 | no | `data[].change_pct` | |
| 12 | `_nats_seq` | UInt64 | no | pipeline | Shared across all items from same message |
| 13 | `_received_at` | Timestamp(us, UTC) | no | pipeline | |

**Data flattening:** A single Kraken message with `data: [{...}, {...}]` produces 2 rows, both sharing the same `_nats_seq`.

**JSON fields NOT in parquet:** `channel`, `type` (message envelope fields)

**Source:** `ssmd-schemas/src/kraken.rs:25-151`

---

### kraken_trade

**Identity:** `schema_name = "kraken_trade"`, `schema_version = "1.0.0"`, `message_type = "trade"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `symbol` | Utf8 | no | `data[].symbol` | |
| 1 | `side` | Utf8 | no | `data[].side` | `buy` or `sell` |
| 2 | `price` | Float64 | no | `data[].price` | |
| 3 | `qty` | Float64 | no | `data[].qty` | |
| 4 | `ord_type` | Utf8 | no | `data[].ord_type` | `market`, `limit`, etc. |
| 5 | `trade_id` | Utf8 | no | `data[].trade_id` | String or integer in JSON, always coerced to Utf8 |
| 6 | `timestamp` | Timestamp(us, UTC) | no | `data[].timestamp` | ISO 8601 string, parsed via `chrono::DateTime::parse_from_rfc3339()` |
| 7 | `_nats_seq` | UInt64 | no | pipeline | |
| 8 | `_received_at` | Timestamp(us, UTC) | no | pipeline | |

**Type conversion:** `trade_id` may arrive as a JSON integer in some API versions; the parser coerces it to string. `timestamp` is an ISO 8601 string (e.g., `2026-02-06T12:00:00.000000Z`) parsed to microseconds.

**Source:** `ssmd-schemas/src/kraken.rs:157-302`

---

## Kraken Futures Schemas

Kraken Futures V1 API uses flat JSON (no `data[]` wrapper): `{"feed": "ticker", "product_id": "PF_XBTUSD", ...}`. Messages with an `event` field (subscription confirmations) are skipped by the detector.

### kraken_futures_ticker

**Identity:** `schema_name = "kraken_futures_ticker"`, `schema_version = "1.0.0"`, `message_type = "ticker"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `product_id` | Utf8 | no | `product_id` | e.g., `PF_XBTUSD`, `PF_ETHUSD` |
| 1 | `bid` | Float64 | no | `bid` | |
| 2 | `bid_size` | Float64 | no | `bid_size` | |
| 3 | `ask` | Float64 | no | `ask` | |
| 4 | `ask_size` | Float64 | no | `ask_size` | |
| 5 | `last` | Float64 | no | `last` | |
| 6 | `volume` | Float64 | no | `volume` | |
| 7 | `volume_quote` | Float64 | yes | `volumeQuote` | **camelCase** in JSON |
| 8 | `open` | Float64 | yes | `open` | |
| 9 | `high` | Float64 | yes | `high` | |
| 10 | `low` | Float64 | yes | `low` | |
| 11 | `change` | Float64 | yes | `change` | |
| 12 | `index_price` | Float64 | yes | `index` | JSON field is `index`, column is `index_price` |
| 13 | `mark_price` | Float64 | yes | `markPrice` | **camelCase** in JSON |
| 14 | `open_interest` | Float64 | yes | `openInterest` | **camelCase** in JSON |
| 15 | `funding_rate` | Float64 | yes | `funding_rate` | |
| 16 | `funding_rate_prediction` | Float64 | yes | `funding_rate_prediction` | |
| 17 | `next_funding_rate_time` | Int64 | yes | `next_funding_rate_time` | Epoch milliseconds (raw, not converted) |
| 18 | `time` | Timestamp(us, UTC) | no | `time` | Epoch milliseconds, converted: `ms * 1000` |
| 19 | `_nats_seq` | UInt64 | no | pipeline | |
| 20 | `_received_at` | Timestamp(us, UTC) | no | pipeline | |

**Field name mismatches:** Several JSON fields use camelCase (`volumeQuote`, `markPrice`, `openInterest`) while parquet columns use snake_case. The `index` JSON field maps to `index_price` column.

**JSON fields NOT in parquet:** `feed` (detection field)

**Source:** `ssmd-schemas/src/kraken_futures.rs:24-189`

---

### kraken_futures_trade

**Identity:** `schema_name = "kraken_futures_trade"`, `schema_version = "1.0.0"`, `message_type = "trade"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `product_id` | Utf8 | no | `product_id` | |
| 1 | `uid` | Utf8 | no | `uid` | Trade UUID |
| 2 | `side` | Utf8 | no | `side` | `buy` or `sell` |
| 3 | `trade_type` | Utf8 | no | `type` | JSON field `type` renamed to `trade_type` (e.g., `fill`) |
| 4 | `seq` | Int64 | no | `seq` | Exchange sequence number |
| 5 | `qty` | Float64 | no | `qty` | |
| 6 | `price` | Float64 | no | `price` | |
| 7 | `time` | Timestamp(us, UTC) | no | `time` | Epoch milliseconds, converted: `ms * 1000` |
| 8 | `_nats_seq` | UInt64 | no | pipeline | |
| 9 | `_received_at` | Timestamp(us, UTC) | no | pipeline | |

**JSON fields NOT in parquet:** `feed` (detection field)

**Source:** `ssmd-schemas/src/kraken_futures.rs:195-332`

---

## Polymarket Schemas

Polymarket messages use flat JSON with `event_type` for detection. Numeric values (prices, sizes) are kept as strings to preserve decimal precision from the CLOB API.

### polymarket_book

**Identity:** `schema_name = "polymarket_book"`, `schema_version = "1.0.0"`, `message_type = "book"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `asset_id` | Utf8 | no | `asset_id` | Condition token ID (long numeric string) |
| 1 | `market` | Utf8 | no | `market` | Condition ID (hex string, e.g., `0x1234abcd`) |
| 2 | `timestamp_ms` | Int64 | yes | `timestamp` | String or int in JSON, parsed via `parse_timestamp_ms()` |
| 3 | `hash` | Utf8 | yes | `hash` | |
| 4 | `bids_json` | Utf8 | no | `buys` or `bids` | JSON-serialized array (tries `buys` first, then `bids`) |
| 5 | `asks_json` | Utf8 | no | `sells` or `asks` | JSON-serialized array (tries `sells` first, then `asks`) |
| 6 | `_nats_seq` | UInt64 | no | pipeline | |
| 7 | `_received_at` | Timestamp(us, UTC) | no | pipeline | |

**Order book storage:** The bids and asks arrays are serialized as JSON strings rather than being flattened into rows. Each entry in the array is `{"price": "0.55", "size": "1000"}`.

**Field aliasing:** Polymarket API sends `buys`/`sells`; the parser also accepts `bids`/`asks` as aliases.

**JSON fields NOT in parquet:** `event_type` (detection field)

**Source:** `ssmd-schemas/src/polymarket.rs:27-139`

---

### polymarket_trade

**Identity:** `schema_name = "polymarket_trade"`, `schema_version = "1.0.0"`, `message_type = "last_trade_price"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `asset_id` | Utf8 | no | `asset_id` | |
| 1 | `market` | Utf8 | no | `market` | |
| 2 | `price` | Utf8 | no | `price` | **String**, not numeric. Preserves decimal precision. |
| 3 | `side` | Utf8 | yes | `side` | `BUY` or `SELL` |
| 4 | `size` | Utf8 | yes | `size` | **String**, not numeric |
| 5 | `fee_rate_bps` | Utf8 | yes | `fee_rate_bps` | **String**, not numeric |
| 6 | `timestamp_ms` | Int64 | yes | `timestamp` | String or int in JSON |
| 7 | `_nats_seq` | UInt64 | no | pipeline | |
| 8 | `_received_at` | Timestamp(us, UTC) | no | pipeline | |

**String-typed numerics:** `price`, `size`, and `fee_rate_bps` are stored as Utf8 strings to match the Polymarket CLOB API format and preserve exact decimal representation.

**JSON fields NOT in parquet:** `event_type`

**Source:** `ssmd-schemas/src/polymarket.rs:145-257`

---

### polymarket_price_change

**Identity:** `schema_name = "polymarket_price_change"`, `schema_version = "1.0.0"`, `message_type = "price_change"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `market` | Utf8 | no | `market` | Parent-level field, repeated per item |
| 1 | `timestamp_ms` | Int64 | yes | `timestamp` | Parent-level, repeated per item |
| 2 | `asset_id` | Utf8 | no | `price_changes[].asset_id` | |
| 3 | `price` | Utf8 | no | `price_changes[].price` | String |
| 4 | `size` | Utf8 | no | `price_changes[].size` | String |
| 5 | `side` | Utf8 | no | `price_changes[].side` | |
| 6 | `hash` | Utf8 | yes | `price_changes[].hash` | |
| 7 | `best_bid` | Utf8 | yes | `price_changes[].best_bid` | String |
| 8 | `best_ask` | Utf8 | yes | `price_changes[].best_ask` | String |
| 9 | `_nats_seq` | UInt64 | no | pipeline | Shared across all items from same message |
| 10 | `_received_at` | Timestamp(us, UTC) | no | pipeline | |

**Flattening:** The JSON has a nested `price_changes[]` array. Each item in the array produces one row in the parquet output. Parent-level fields (`market`, `timestamp`) are repeated for every item row. Items missing required fields (`asset_id`, `price`, `size`, `side`) are skipped individually without affecting other items in the same message.

**JSON fields NOT in parquet:** `event_type`, `price_changes` (array structure itself, flattened into rows)

**Source:** `ssmd-schemas/src/polymarket.rs:262-411`

---

### polymarket_best_bid_ask

**Identity:** `schema_name = "polymarket_best_bid_ask"`, `schema_version = "1.0.0"`, `message_type = "best_bid_ask"`

| # | Column | Arrow Type | Nullable | JSON Source | Notes |
|---|--------|-----------|----------|------------|-------|
| 0 | `market` | Utf8 | no | `market` | |
| 1 | `asset_id` | Utf8 | no | `asset_id` | |
| 2 | `best_bid` | Utf8 | yes | `best_bid` | String |
| 3 | `best_ask` | Utf8 | yes | `best_ask` | String |
| 4 | `spread` | Utf8 | yes | `spread` | String |
| 5 | `timestamp_ms` | Int64 | yes | `timestamp` | String or int in JSON |
| 6 | `_nats_seq` | UInt64 | no | pipeline | |
| 7 | `_received_at` | Timestamp(us, UTC) | no | pipeline | |

**JSON fields NOT in parquet:** `event_type`

**Source:** `ssmd-schemas/src/polymarket.rs:417-518`

---

## Cross-Cutting Details

### Parquet File Metadata

Every parquet file written by `ssmd-parquet-gen` embeds key-value metadata in the file footer via `WriterProperties`:

| Key | Example Value | Description |
|-----|--------------|-------------|
| `ssmd.schema_name` | `kalshi_ticker` | Schema identity from `MessageSchema::schema_name()` |
| `ssmd.schema_version` | `1.1.0` | Semver version from `MessageSchema::schema_version()` |
| `created_by` | `ssmd-parquet-gen` | Distinguishes from legacy inline parquet written by `ssmd-archiver` |

**Source:** `ssmd-parquet-gen/src/processor.rs:364-388` (`write_parquet_to_bytes()`)

---

### File Naming & GCS Layout

```
gs://{bucket}/{prefix}/{feed}/{stream}/{date}/{msg_type}_{HHMM}00.parquet
```

Examples:
```
gs://ssmd-data/kalshi/kalshi/crypto/2026-02-12/ticker_1400.parquet
gs://ssmd-data/kalshi/kalshi/crypto/2026-02-12/trade_1400.parquet
gs://ssmd-data/kraken-futures/kraken-futures/futures/2026-02-12/ticker_0800.parquet
gs://ssmd-data/polymarket/polymarket/markets/2026-02-12/book_1800.parquet
gs://ssmd-data/polymarket/polymarket/markets/2026-02-12/price_change_1800.parquet
```

**Hour grouping:** JSONL.gz filenames like `1415.jsonl.gz` are grouped by their first 2 digits (hour). All files in the same hour (e.g., `1400.jsonl.gz`, `1415.jsonl.gz`, `1430.jsonl.gz`, `1445.jsonl.gz`) are combined into a single parquet file per message type with the hour key `14` producing `{msg_type}_1400.parquet`.

**Source JSONL.gz path:**
```
gs://{bucket}/{prefix}/{feed}/{stream}/{date}/{HHMM}.jsonl.gz
```

---

### Pipeline-Injected Columns

Every schema includes two columns added by the pipeline (not from exchange JSON):

| Column | Type | Description |
|--------|------|-------------|
| `_nats_seq` | UInt64, not null | Real NATS JetStream sequence number, injected into JSONL by the archiver at write time. For JSONL files written before v0.9.10, falls back to a 1-indexed line counter. |
| `_received_at` | Timestamp(us, UTC), not null | Per-message receive timestamp (epoch microseconds), injected into JSONL by the archiver at write time. For JSONL files written before v0.9.10, falls back to the hour boundary timestamp (e.g., `2026-02-12T14:00:00Z`). |

**Implementation:** The archiver injects `_nats_seq` and `_received_at` as JSON fields into each JSONL line via byte-level injection (no serde round-trip). `ssmd-parquet-gen` extracts these from the JSON with backward-compatible fallback for older files.

**Source:** Archiver injection: `ssmd-archiver/src/writer.rs`. Parquet extraction: `ssmd-parquet-gen/src/processor.rs`

---

### Schema Versioning

Current schema versions:

| Schema | Version | Last Changed |
|--------|---------|-------------|
| `kalshi_ticker` | 1.3.0 | Added `_shard_id`, `exchange_clock` columns |
| `kalshi_trade` | 1.3.0 | Added `_shard_id`, `exchange_seq` columns, `yes_price`/`taker_side` aliasing |
| `kalshi_lifecycle` | 1.0.0 | Initial |
| `kraken_ticker` | 1.0.0 | Initial |
| `kraken_trade` | 1.0.0 | Initial |
| `kraken_futures_ticker` | 1.0.0 | Initial |
| `kraken_futures_trade` | 1.0.0 | Initial |
| `polymarket_book` | 1.0.0 | Initial |
| `polymarket_trade` | 2.1.0 | Made `size` non-nullable |
| `polymarket_price_change` | 2.0.0 | Added `best_bid`, `best_ask` columns |
| `polymarket_best_bid_ask` | 2.0.0 | Added `spread` column |

Versions are embedded in parquet metadata (`ssmd.schema_version`). When a schema changes:
1. Bump the version in the Rust source
2. Old parquet files retain their version tag
3. To regenerate with the new schema: delete old parquet files, then re-run `ssmd-parquet-gen` for affected dates

There is no automatic migration between schema versions.

---

### WriterProperties

All parquet files are written with these settings:

| Property | Value |
|----------|-------|
| Compression | Snappy |
| Max row group size | 100,000 rows |
| Data page size limit | 1 MB |
| Statistics | Chunk-level (per row group) |
| Created by | `ssmd-parquet-gen` |

**Source:** `ssmd-parquet-gen/src/processor.rs:365-381`

---

## Type Conversion Summary

| Conversion | Feeds | Details |
|-----------|-------|---------|
| Unix seconds -> Timestamp(us) | Kalshi | `ts * 1_000_000` |
| Epoch milliseconds -> Timestamp(us) | Kraken Futures | `time * 1_000` |
| ISO 8601 string -> Timestamp(us) | Kraken Spot | `chrono::DateTime::parse_from_rfc3339()` |
| String -> Int64 (timestamp_ms) | Polymarket | `parse_timestamp_ms()` handles both string and int JSON values |
| JSON object -> Utf8 string | Kalshi lifecycle | `additional_metadata` serialized via `to_string()` |
| JSON array -> Utf8 string | Polymarket book | `buys`/`sells` arrays serialized as JSON strings |
| camelCase -> snake_case | Kraken Futures | `volumeQuote` -> `volume_quote`, `markPrice` -> `mark_price`, etc. |
| Field renaming | Multiple | `msg.price` -> `last_price` (Kalshi ticker), `json.type` -> `trade_type` (Kraken Futures trade), `json.index` -> `index_price` |
| Integer -> Utf8 | Kraken Spot | `trade_id` may arrive as integer, coerced to string |
