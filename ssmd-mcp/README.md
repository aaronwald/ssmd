# ssmd-mcp

MCP server for querying ssmd market data. Provides 5 tools for trade/price queries, market lookups, feed discovery, and freshness checks.

All queries are executed server-side by [ssmd-data-ts](../ssmd-agent/src/server/) via its DuckDB/parquet endpoints. The MCP server is a pure API client — no local GCS credentials or DuckDB required.

## Prerequisites

- Python >= 3.11
- [uv](https://docs.astral.sh/uv/) (recommended) or pip
- `SSMD_API_URL` and `SSMD_API_KEY` environment variables

## Setup

```bash
# Create .env file
cat > .env <<EOF
SSMD_API_URL=https://ssmd-data-ts.your-domain.com
SSMD_API_KEY=your-api-key
EOF

# Install dependencies
uv sync
```

## Usage with Claude Code

Add to your `.mcp.json`:

```json
{
  "mcpServers": {
    "ssmd": {
      "command": "uv",
      "args": ["run", "--directory", "/path/to/ssmd/ssmd-mcp", "ssmd-mcp"]
    }
  }
}
```

Environment variables can also be passed via the `env` key in .mcp.json.

## Tools

| Tool | Description |
|------|-------------|
| `query_trades` | Trade aggregation by ticker — count, volume, price range. Feeds: kalshi, kraken-futures, polymarket. |
| `query_prices` | Latest price snapshots per instrument. Kalshi: yes/no bid/ask. Kraken: bid/ask/funding. Polymarket: best bid/ask/spread. |
| `lookup_market` | Market metadata lookup by ID (ticker, product_id, condition_id). Cached per session. |
| `list_feeds` | List available feeds with catalog info (dates, file counts, schemas). |
| `check_freshness` | Check data freshness per feed. Flags stale feeds (>7 hours old). |

## Architecture

```
Claude Code ──MCP stdio──▶ ssmd-mcp (Python)
                              │
                              ▼
                        ssmd-data-ts (GKE)
                              │
                         DuckDB + httpfs
                              │
                              ▼
                        GCS parquet files
```

The ssmd-data-ts server runs DuckDB with httpfs configured for GCS via Workload Identity. All parquet query logic lives server-side — the MCP server just forwards requests.

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `SSMD_API_URL` | Yes | Base URL of ssmd-data-ts API |
| `SSMD_API_KEY` | Yes | API key with `datasets:read` scope |

## Development

```bash
# Run directly
uv run ssmd-mcp

# Or install and run
uv pip install -e .
ssmd-mcp
```
