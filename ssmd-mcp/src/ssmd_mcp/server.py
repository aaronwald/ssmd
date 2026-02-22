"""ssmd-mcp: MCP server for querying ssmd market data."""

import asyncio
import json
import logging

from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import Tool, TextContent

from ssmd_mcp.config import load_config, Config
from ssmd_mcp.tools import (
    query_trades,
    query_prices,
    lookup_market,
    list_feeds,
    check_freshness,
    query_events,
    query_volume,
    list_api_keys,
    query_key_usage,
)

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

TOOLS = [
    Tool(
        name="query_trades",
        description=(
            "Query trade data from ssmd parquet files. Groups by ticker/instrument, "
            "counts trades, sums volume, and returns price range (min/max/avg). "
            "Kalshi prices are converted from cents to dollars. "
            "Feeds: kalshi, kraken-futures, polymarket."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "feed": {
                    "type": "string",
                    "description": "Feed name: kalshi, kraken-futures, or polymarket",
                    "enum": ["kalshi", "kraken-futures", "polymarket"],
                },
                "date": {
                    "type": "string",
                    "description": "Date in YYYY-MM-DD format. Defaults to today.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max number of tickers to return. Default 20.",
                    "default": 20,
                },
            },
            "required": ["feed"],
        },
    ),
    Tool(
        name="query_prices",
        description=(
            "Get latest price snapshots per instrument from ticker/bid-ask parquet files. "
            "Returns the most recent snapshot for each instrument. "
            "Kalshi: yes/no bid/ask/last. Kraken: bid/ask/last/funding_rate. "
            "Polymarket: best_bid/best_ask/spread."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "feed": {
                    "type": "string",
                    "description": "Feed name: kalshi, kraken-futures, or polymarket",
                    "enum": ["kalshi", "kraken-futures", "polymarket"],
                },
                "date": {
                    "type": "string",
                    "description": "Date in YYYY-MM-DD format. Defaults to today.",
                },
                "hour": {
                    "type": "string",
                    "description": "Hour in HHMM format (e.g., '1400'). Defaults to most recent.",
                },
            },
            "required": ["feed"],
        },
    ),
    Tool(
        name="lookup_market",
        description=(
            "Look up market metadata from ssmd-data-ts API (GET /v1/markets/lookup). "
            "Returns id, feed, name, event, series, status, closeTime, volume, volumeUnit, openInterest. "
            "Searches Kalshi tickers, Kraken pair_ids, Polymarket condition/token IDs. "
            "Results cached in-memory for session."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "ids": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Market IDs to look up (e.g., ticker symbols, product IDs).",
                },
                "feed": {
                    "type": "string",
                    "description": "Optional feed filter: kalshi, kraken-futures, or polymarket.",
                },
            },
            "required": ["ids"],
        },
    ),
    Tool(
        name="list_feeds",
        description=(
            "List available data feeds with GCS date availability and catalog info. "
            "Shows feed names, parquet types, and recent dates with data."
        ),
        inputSchema={
            "type": "object",
            "properties": {},
        },
    ),
    Tool(
        name="check_freshness",
        description=(
            "Check data freshness for feeds. Finds the newest parquet/JSONL files "
            "in GCS, reports their age, and flags feeds that are stale (>7 hours old)."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "feed": {
                    "type": "string",
                    "description": "Optional: check a specific feed. Otherwise checks all feeds.",
                },
            },
        },
    ),
    Tool(
        name="query_events",
        description=(
            "Query event-level trade summaries from ssmd parquet files. "
            "Groups markets by parent event (Kalshi events, Polymarket conditions) "
            "and aggregates trade activity. Returns event name, close time, market count, "
            "total volume, and top markets. Kraken has no event hierarchy — returns per-instrument. "
            "Volume units differ by feed: contracts (Kalshi), USD (Polymarket), base currency (Kraken)."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "feed": {
                    "type": "string",
                    "enum": ["kalshi", "kraken-futures", "polymarket"],
                    "description": "Feed name",
                },
                "date": {
                    "type": "string",
                    "description": "Date in YYYY-MM-DD format. Defaults to today.",
                },
                "limit": {
                    "type": "integer",
                    "default": 20,
                    "description": "Max events to return. Default 20.",
                },
            },
            "required": ["feed"],
        },
    ),
    Tool(
        name="query_volume",
        description=(
            "Get volume summary across feeds for a date. Returns per-feed trade counts, "
            "total volume (in feed-native units), active ticker count, and top tickers. "
            "Does NOT sum across feeds — volumes use different units "
            "(contracts, USD, base currency)."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "date": {
                    "type": "string",
                    "description": "Date in YYYY-MM-DD format. Defaults to today.",
                },
                "feed": {
                    "type": "string",
                    "description": "Optional: filter to a single feed.",
                },
            },
        },
    ),
    Tool(
        name="list_api_keys",
        description=(
            "List all API keys with metadata. Shows prefix, name, user email, scopes, "
            "rate limit tier, allowed feeds, date range, expiration, and last used time. "
            "Requires admin scope. Never returns key secrets."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "include_revoked": {
                    "type": "boolean",
                    "description": "Include revoked keys. Default false.",
                    "default": False,
                },
            },
        },
    ),
    Tool(
        name="query_key_usage",
        description=(
            "Query API key usage statistics. Returns per-key rate limit metrics "
            "(requests in current window, limit, tier) and LLM token usage "
            "(prompt/completion tokens, per-model costs, daily breakdown). "
            "Optionally filter to a specific key by prefix. Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "key_prefix": {
                    "type": "string",
                    "description": "Optional: filter to a specific key prefix.",
                },
            },
        },
    ),
]


def _run_tool(cfg: Config, name: str, arguments: dict) -> str:
    """Dispatch tool call to implementation."""
    if name == "query_trades":
        return query_trades(
            cfg,
            feed=arguments["feed"],
            date_str=arguments.get("date"),
            limit=arguments.get("limit", 20),
        )
    elif name == "query_prices":
        return query_prices(
            cfg,
            feed=arguments["feed"],
            date_str=arguments.get("date"),
            hour=arguments.get("hour"),
        )
    elif name == "lookup_market":
        return lookup_market(
            cfg,
            ids=arguments["ids"],
            feed=arguments.get("feed"),
        )
    elif name == "list_feeds":
        return list_feeds(cfg)
    elif name == "check_freshness":
        return check_freshness(
            cfg,
            feed=arguments.get("feed"),
        )
    elif name == "query_events":
        return query_events(
            cfg,
            feed=arguments["feed"],
            date_str=arguments.get("date"),
            limit=arguments.get("limit", 20),
        )
    elif name == "query_volume":
        return query_volume(
            cfg,
            date_str=arguments.get("date"),
            feed=arguments.get("feed"),
        )
    elif name == "list_api_keys":
        return list_api_keys(
            cfg,
            include_revoked=arguments.get("include_revoked", False),
        )
    elif name == "query_key_usage":
        return query_key_usage(
            cfg,
            key_prefix=arguments.get("key_prefix"),
        )
    else:
        return json.dumps({"error": f"Unknown tool: {name}"})


async def serve() -> None:
    """Run the MCP server with stdio transport."""
    cfg = load_config()
    server = Server("ssmd-mcp")

    @server.list_tools()
    async def handle_list_tools() -> list[Tool]:
        return TOOLS

    @server.call_tool()
    async def handle_call_tool(name: str, arguments: dict | None) -> list[TextContent]:
        arguments = arguments or {}
        try:
            result = _run_tool(cfg, name, arguments)
        except Exception as e:
            logger.exception("Tool %s failed", name)
            result = json.dumps({"error": str(e)})
        return [TextContent(type="text", text=result)]

    async with stdio_server() as (read_stream, write_stream):
        await server.run(read_stream, write_stream, server.create_initialization_options())


def main() -> None:
    """Entry point for ssmd-mcp server."""
    asyncio.run(serve())


if __name__ == "__main__":
    main()
