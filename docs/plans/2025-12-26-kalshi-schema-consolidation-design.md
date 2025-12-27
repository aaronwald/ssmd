# Kalshi Schema Consolidation Design

> Full schema documentation, secmaster integration, and new builders for ssmd-agent.

## Goals

1. **Document existing schemas** - Comprehensive reference for Kalshi data structures
2. **Design new builders** - Price history, volume profile state builders
3. **Plan secmaster integration** - Event/market hierarchy in ssmd
4. **Enable use cases**: Market discovery, signal filtering, fee-aware trading, portfolio context

## Data Model

### Events (container for related markets)

| Field | Type | Description |
|-------|------|-------------|
| event_ticker | string | Primary key (e.g., "INXD-25JAN01") |
| title | string | Human-readable name |
| category | string | E.g., "Economics", "Politics", "Sports" |
| series_ticker | string | Groups related events (e.g., "INXD") |
| strike_date | timestamp | When event resolves |
| mutually_exclusive | bool | Markets are mutually exclusive |
| status | enum | open, closed, settled |

### Markets (tradeable contracts)

| Field | Type | Description |
|-------|------|-------------|
| ticker | string | Primary key (e.g., "INXD-25JAN01-B4550") |
| event_ticker | string | FK to event |
| title | string | Contract description |
| status | enum | open, closed, settled |
| close_time | timestamp | When trading closes |
| yes_bid, yes_ask | int | Cents (0-100) |
| no_bid, no_ask | int | Cents (0-100) |
| last_price | int | Last trade price |
| volume | int | Total contracts traded |
| volume_24h | int | 24-hour volume |
| open_interest | int | Outstanding contracts |

### Fees

| Field | Type | Description |
|-------|------|-------------|
| tier | string | Fee tier (e.g., "default", "vip") |
| maker_fee | decimal | Fee for providing liquidity |
| taker_fee | decimal | Fee for taking liquidity |

### Hierarchy

```
Series (INXD) → Events (INXD-25JAN01) → Markets (INXD-25JAN01-B4550)
```

## Stream Message Types (WebSocket)

### trade
```json
{
  "type": "trade",
  "ticker": "INXD-25JAN01-B4550",
  "price": 45,
  "count": 10,
  "side": "yes",
  "taker_side": "buy",
  "ts": 1703635200000
}
```

### ticker
```json
{
  "type": "ticker",
  "ticker": "INXD-25JAN01-B4550",
  "yes_bid": 44,
  "yes_ask": 46,
  "no_bid": 54,
  "no_ask": 56,
  "last_price": 45,
  "volume": 1234,
  "open_interest": 5678,
  "ts": 1703635200000
}
```

### orderbook
```json
{
  "type": "orderbook",
  "ticker": "INXD-25JAN01-B4550",
  "yes_bid": 44,
  "yes_ask": 46,
  "no_bid": 54,
  "no_ask": 56,
  "ts": 1703635200000
}
```

## API Extensions (ssmd-data)

### GET /markets

Query parameters:
- `category` - Filter by category
- `status` - open, closed, settled
- `series` - Filter by series ticker
- `closing_before` - ISO timestamp
- `closing_after` - ISO timestamp
- `limit` - Max results (default 100)

Returns markets with event metadata joined.

### GET /markets/{ticker}

Returns full market details with event metadata.

### GET /events

Query parameters:
- `category` - Filter by category
- `status` - open, closed, settled
- `series` - Filter by series ticker

Returns events with market count.

### GET /events/{event_ticker}

Returns event with list of markets.

### GET /fees

Query parameters:
- `tier` - Fee tier (default: "default")

Returns maker/taker fees.

## Agent Tools

### New Tools

```typescript
list_markets({ category?, status?, series?, closing_before?, closing_after?, limit? })
get_market({ ticker })
list_events({ category?, status?, series? })
get_fees({ tier? })
```

### New Builders

**price_history_builder**
- Derived: last, vwap, high, low, returns, volatility
- Consumes: trade messages
- Window: Configurable (e.g., 100 trades)

**volume_profile_builder**
- Derived: buyVolume, sellVolume, totalVolume, ratio, average
- Consumes: trade messages
- Window: Configurable time window

### Workflow Example

1. `list_markets({ category: "Economics", status: "open", closing_before: "2025-12-27" })`
2. `list_tickers({ feed: "kalshi", date: "2025-12-26" })`
3. Intersect to get tickers with both secmaster info and stream data
4. Build signals filtered to those tickers

## Sync Architecture

### Phase 1: Direct Postgres

```
Kalshi REST API → ssmd secmaster sync → PostgreSQL
                  (scheduled job)
```

- Full sync: 30-day window, daily at market open
- Incremental sync: 1-day window, every 15 minutes
- Soft deletes for removed markets
- Port sync logic from tradfiportal's `kalshi-secmaster` crate

### Phase 2: CDC + Cache

```
PostgreSQL → Debezium → NATS → Redis cache
                (CDC)
```

- Real-time cache updates via CDC
- ssmd-data queries cache first, falls back to Postgres

## CLI Commands

```bash
ssmd secmaster sync              # Full sync
ssmd secmaster sync --incremental # Quick refresh
ssmd secmaster list --category=Economics --status=open
ssmd secmaster show INXD-25JAN01-B4550
```

## Implementation Phases

### Phase 1: Core Secmaster (MVP)
- Port `kalshi-secmaster` sync logic to ssmd-rust
- PostgreSQL schema (events, markets tables)
- ssmd-data API endpoints
- Agent tools
- CLI commands

### Phase 2: Builders
- price_history_builder
- volume_profile_builder
- Updated skills

### Phase 3: Cache + CDC
- Debezium setup
- Redis cache
- Cache-first queries

## Not Building (YAGNI)

- Real-time secmaster updates via WebSocket (REST sync sufficient)
- Historical secmaster snapshots (current state only)
- Multi-exchange support (Kalshi only for now)
