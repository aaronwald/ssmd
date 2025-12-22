# ssmd: Kalshi Design - Data Flow

## Wire Formats

| Path | Format | Rationale |
|------|--------|-----------|
| Kalshi → Connector | JSON (WebSocket) | Exchange native format |
| Connector → NATS | Cap'n Proto | Compact, schema-enforced, learning goal |
| NATS → Archiver | Cap'n Proto | Consistent internal format |
| Gateway → Clients | JSON | Human/agent readable |
| Raw storage | JSONL (compressed) | Preserve original exchange data |
| Normalized storage | Cap'n Proto | Compact, typed, replayable |

## NATS Subjects

```
kalshi.raw.{event_type}           # Raw events from connector
kalshi.normalized.{event_type}    # Normalized events
kalshi.trade.{ticker}             # Per-symbol trade stream
kalshi.orderbook.{ticker}         # Per-symbol orderbook updates
```

JetStream consumers provide replay capability and persistence.

## Cap'n Proto Schema

```capnp
@0xabcdef1234567890;

struct Trade {
  timestamp @0 :UInt64;        # Unix nanos
  ticker @1 :Text;
  price @2 :Float64;
  size @3 :UInt32;
  side @4 :Side;
  tradeId @5 :Text;
}

enum Side {
  buy @0;
  sell @1;
}

struct OrderBookUpdate {
  timestamp @0 :UInt64;
  ticker @1 :Text;
  bids @2 :List(Level);
  asks @3 :List(Level);
}

struct Level {
  price @0 :Float64;
  size @1 :UInt32;
}

struct MarketStatus {
  timestamp @0 :UInt64;
  ticker @1 :Text;
  status @2 :Status;
}

enum Status {
  open @0;
  closed @1;
  halted @2;
}
```

## Storage Layout

### S3 Buckets

```
ssmd-raw/
  kalshi/
    2025/12/14/
      trades-00.jsonl.zst
      trades-01.jsonl.zst
      orderbook-00.jsonl.zst
      manifest.json

ssmd-normalized/
  kalshi/
    v1/
      trade/
        2025/12/14/
          {ticker}/
            data.capnp.zst
      orderbook/
        2025/12/14/
          {ticker}/
            data.capnp.zst
      manifest.json
```

### Raw Format

Compressed JSONL preserving original Kalshi messages:

```json
{"ts":1702540800000,"type":"trade","data":{"ticker":"INXD-25-B4000","price":0.45,"count":10}}
{"ts":1702540800100,"type":"orderbook","data":{"ticker":"INXD-25-B4000","yes_bid":0.44,"yes_ask":0.46}}
```

### Data Keying Strategy

All data is keyed by environment prefix to support rapid teardown/rebuild cycles. This enables quick iteration during development and clean separation between environments.

**Key Structure:**

| Store | Key Pattern | Example |
|-------|-------------|---------|
| S3 | `{bucket}/{env}/{feed}/{date}/` | `ssmd-raw/kalshi-dev/kalshi/2025/12/14/` |
| NATS | `{env}.{feed}.{type}.{symbol}` | `kalshi-dev.kalshi.trade.BTCUSD` |
| Redis | `{env}:{feed}:{type}:{key}` | `kalshi-dev:kalshi:price:BTCUSD` |

**Teardown Operations:**

```bash
# Tear down a single environment (all data)
ssmd env teardown kalshi-dev
#   Deleting S3 prefix: ssmd-raw/kalshi-dev/
#   Deleting S3 prefix: ssmd-normalized/kalshi-dev/
#   Deleting NATS streams: kalshi-dev.*
#   Deleting Redis keys: kalshi-dev:*
# Environment kalshi-dev torn down.

# Tear down specific date only
ssmd env teardown kalshi-dev --date 2025-12-14

# Preview teardown (dry run)
ssmd env teardown kalshi-dev --dry-run
```

**Benefits:**
- **Isolation** - Dev/staging/prod data never mix
- **Fast cleanup** - Single prefix delete instead of scanning
- **Reproducibility** - Rebuild from scratch with same config
- **Cost control** - Easy to purge old test environments

## Gateway API

### WebSocket

```
ws://gateway.ssmd.local/v1/stream?symbols=INXD-25-B4000,KXBTC-25DEC31

# Subscribe message
{"action": "subscribe", "symbols": ["INXD-25-B4000"]}

# Trade event
{"type": "trade", "ticker": "INXD-25-B4000", "price": 0.45, "size": 10, "side": "buy", "ts": 1702540800000}

# Orderbook snapshot
{"type": "orderbook", "ticker": "INXD-25-B4000", "bids": [[0.44, 100]], "asks": [[0.46, 150]], "ts": 1702540800000}
```

### REST

```
GET /v1/markets                    # List all markets
GET /v1/markets/{ticker}           # Market details
GET /v1/markets/{ticker}/trades    # Recent trades
GET /v1/health                     # System health
```

## Data Pipeline

```
┌─────────────┐
│   Kalshi    │
│  WebSocket  │
└──────┬──────┘
       │ JSON
       ▼
┌─────────────┐     ┌─────────────┐
│  Connector  │────▶│ Raw Archive │
│   (Rust)    │     │   (JSONL)   │
└──────┬──────┘     └──────┬──────┘
       │ Cap'n Proto       │
       ▼                   ▼
┌─────────────┐     ┌─────────────┐
│    NATS     │     │     S3      │
│  JetStream  │     │  (ssmd-raw) │
└──────┬──────┘     └─────────────┘
       │
       ├────────────────┐
       │                │
       ▼                ▼
┌─────────────┐  ┌─────────────┐
│  Archiver   │  │   Gateway   │
│   (Rust)    │  │   (Rust)    │
└──────┬──────┘  └──────┬──────┘
       │ Cap'n Proto    │ JSON
       ▼                ▼
┌─────────────┐  ┌─────────────┐
│     S3      │  │   Clients   │
│(normalized) │  │ (WS + REST) │
└─────────────┘  └─────────────┘
```

## Manifest Files

Each data directory includes a manifest for inventory tracking:

```json
{
  "feed": "kalshi",
  "date": "2025-12-14",
  "data_type": "raw",
  "schema_version": null,
  "status": "complete",
  "record_count": 1234567,
  "byte_size": 523456789,
  "first_timestamp": "2025-12-14T00:00:01.234Z",
  "last_timestamp": "2025-12-14T23:59:59.987Z",
  "gaps": [],
  "quality_score": 1.0,
  "connector_version": "0.1.0",
  "created_at": "2025-12-15T00:05:00Z"
}
```

Manifests enable:
- Data inventory without scanning all files
- Quality tracking and gap detection
- Provenance (which connector version produced this data)
- Reproducible backtesting
