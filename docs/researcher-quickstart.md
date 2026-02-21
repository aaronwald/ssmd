# Researcher Data Access Quickstart

Access ssmd parquet market data via the API or MCP tools.

**API endpoint**: `https://api.varshtat.com`
**Schema reference**: [docs/schemas/parquet-schemas.md](schemas/parquet-schemas.md)

## Prerequisites

- An API key with `datasets:read` scope (provided by admin)
- DuckDB, Python/pandas, or any HTTP client

## 1. List Available Feeds

```bash
curl -H "X-API-Key: $API_KEY" https://api.varshtat.com/v1/data/feeds
```

```json
{
  "feeds": [
    { "name": "kalshi", "stream": "crypto", "messageTypes": ["ticker", "trade", "market_lifecycle_v2"] },
    { "name": "kraken-futures", "stream": "futures", "messageTypes": ["ticker", "trade"] },
    { "name": "polymarket", "stream": "markets", "messageTypes": ["last_trade_price", "price_change", "book", "best_bid_ask"] }
  ]
}
```

## 2. Get Signed URLs

```bash
# Single day, all message types
curl -H "X-API-Key: $API_KEY" \
  "https://api.varshtat.com/v1/data/download?feed=kalshi&from=2026-02-15&to=2026-02-15"

# Filter by type
curl -H "X-API-Key: $API_KEY" \
  "https://api.varshtat.com/v1/data/download?feed=kalshi&from=2026-02-15&to=2026-02-15&type=ticker"

# Multi-day range (max 7 days)
curl -H "X-API-Key: $API_KEY" \
  "https://api.varshtat.com/v1/data/download?feed=kalshi&from=2026-02-10&to=2026-02-15&expires=6h"
```

Response:

```json
{
  "feed": "kalshi",
  "from": "2026-02-15",
  "to": "2026-02-15",
  "type": null,
  "files": [
    {
      "path": "kalshi/crypto/2026-02-15/ticker_0000.parquet",
      "name": "ticker_0000.parquet",
      "type": "ticker",
      "hour": "0000",
      "bytes": 1234567,
      "signedUrl": "https://storage.googleapis.com/...",
      "expiresAt": "2026-02-16T00:00:00.000Z"
    }
  ],
  "expiresIn": "12h"
}
```

## 3. Download Files

```bash
# Download a single file
curl -o ticker_0000.parquet "<signed_url>"

# Download all files from response
curl -s -H "X-API-Key: $API_KEY" \
  "https://api.varshtat.com/v1/data/download?feed=kalshi&from=2026-02-15&to=2026-02-15" \
  | jq -r '.files[].signedUrl' \
  | xargs -n1 -P4 curl -O
```

## 4. Query with DuckDB

```sql
-- Direct query from signed URLs (no download needed)
SELECT * FROM read_parquet('https://storage.googleapis.com/...')
LIMIT 10;

-- Query downloaded files
SELECT * FROM read_parquet('ticker_*.parquet')
WHERE ts BETWEEN '2026-02-15 14:00:00' AND '2026-02-15 15:00:00';

-- Aggregate across all hours
SELECT
  date_trunc('hour', ts) AS hour,
  count(*) AS ticks,
  avg(yes_price) AS avg_price
FROM read_parquet('ticker_*.parquet')
GROUP BY 1
ORDER BY 1;
```

## 5. Query with Python/pandas

```python
import pandas as pd
import requests

API_KEY = "sk_live_..."
API_HOST = "https://api.varshtat.com"

# Get signed URLs
resp = requests.get(
    f"{API_HOST}/v1/data/download",
    params={"feed": "kalshi", "from": "2026-02-15", "to": "2026-02-15"},
    headers={"X-API-Key": API_KEY},
)
files = resp.json()["files"]

# Read all parquet files into one DataFrame
dfs = [pd.read_parquet(f["signedUrl"]) for f in files]
df = pd.concat(dfs, ignore_index=True)

print(f"Loaded {len(df)} rows from {len(files)} files")
print(df.head())
```

## 6. MCP Setup (Claude Code / Claude Desktop)

The ssmd MCP server lets you query market data directly from Claude. It connects to the same API.

### Install

```bash
git clone https://github.com/aaronwald/ssmd.git
cd ssmd/ssmd-mcp
uv sync
```

### Configure environment

Create `ssmd/ssmd-mcp/.env`:

```bash
SSMD_API_URL=https://api.varshtat.com
SSMD_API_KEY=sk_live_...
```

### Add to Claude Code

Create or edit `.mcp.json` in your project root:

```json
{
  "mcpServers": {
    "ssmd": {
      "command": "uv",
      "args": ["run", "--directory", "/path/to/ssmd/ssmd-mcp", "ssmd-mcp"],
      "env": {
        "SSMD_API_URL": "https://api.varshtat.com",
        "SSMD_API_KEY": "sk_live_..."
      }
    }
  }
}
```

### Add to Claude Desktop

Edit `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or `%APPDATA%/Claude/claude_desktop_config.json` (Windows):

```json
{
  "mcpServers": {
    "ssmd": {
      "command": "uv",
      "args": ["run", "--directory", "/path/to/ssmd/ssmd-mcp", "ssmd-mcp"],
      "env": {
        "SSMD_API_URL": "https://api.varshtat.com",
        "SSMD_API_KEY": "sk_live_..."
      }
    }
  }
}
```

### Available MCP Tools

| Tool | Description |
|------|-------------|
| `query_trades` | Trade aggregation by ticker — count, volume, price range |
| `query_prices` | Latest price snapshots per instrument |
| `query_events` | Event-level trade summaries — groups markets by parent event |
| `query_volume` | Cross-feed daily volume summary |
| `lookup_market` | Market metadata lookup by ID |
| `list_feeds` | Available feeds with date ranges |
| `check_freshness` | Data freshness per feed |

### Example MCP queries

Once configured, ask Claude:

- "What were the most traded Kalshi markets today?"
- "Show me Kraken futures prices"
- "How fresh is the polymarket data?"
- "Look up market KXBTCD-26FEB28-T105249.99"

## File Naming Convention

Parquet files follow the pattern: `{type}_{HHMM}.parquet`

- `type` — message type (e.g., `ticker`, `trade`, `book`)
- `HHMM` — hour in UTC (e.g., `0000`, `1430`)

Example: `ticker_1430.parquet` contains ticker messages from 14:30-15:29 UTC.

## Parquet Schemas

Full column definitions for all feeds and message types are in the [Parquet Schema Reference](schemas/parquet-schemas.md).

Summary of available data:

| Feed | Message Type | Key Columns |
|------|-------------|-------------|
| kalshi | ticker | `market_ticker`, `yes_bid`, `yes_ask`, `last_price`, `volume`, `ts` |
| kalshi | trade | `market_ticker`, `price`, `count`, `side`, `trade_id`, `ts` |
| kalshi | market_lifecycle_v2 | `market_ticker`, `event_type`, `open_ts`, `close_ts` |
| kraken-futures | ticker | `product_id`, `bid`, `ask`, `last`, `funding_rate`, `mark_price`, `time` |
| kraken-futures | trade | `product_id`, `price`, `qty`, `side`, `uid`, `time` |
| polymarket | last_trade_price | `asset_id`, `market`, `price`, `size`, `side`, `timestamp_ms` |
| polymarket | book | `asset_id`, `market`, `bids_json`, `asks_json`, `timestamp_ms` |
| polymarket | price_change | `asset_id`, `market`, `price`, `size`, `side`, `best_bid`, `best_ask` |
| polymarket | best_bid_ask | `asset_id`, `market`, `best_bid`, `best_ask`, `spread` |

All schemas include pipeline-injected columns `_nats_seq` (NATS sequence) and `_received_at` (receive timestamp).

## Limits

| Constraint | Value |
|-----------|-------|
| Max date range per request | 7 days |
| Max files per request | 200 |
| URL expiry range | 1-12 hours |
| Default URL expiry | 12 hours |

## Key Expiration

Your API key may have an expiration date and feed/date restrictions. Check with your admin if requests return `401 API key expired` or `403 Feed not authorized`.
