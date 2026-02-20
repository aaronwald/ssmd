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
            "Look up market metadata from ssmd-data-ts API (GET /v1/markets). "
            "Returns market name, description, status, and other metadata. "
            "Results are cached in-memory for the session."
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
