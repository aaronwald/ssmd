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
    query_snap,
    lookup_market,
    list_feeds,
    check_freshness,
    query_events,
    query_volume,
    browse_categories,
    browse_series,
    browse_events,
    browse_markets,
    secmaster_stats,
    search_markets,
    search_events,
    search_pairs,
    search_conditions,
    search_lifecycle,
    get_fees,
    list_api_keys,
    query_key_usage,
    harman_sessions,
    harman_orders,
    harman_fills,
    harman_order_timeline,
    harman_exchange_audit,
    harman_settlements,
    query_market_lifecycle,
    list_pipelines,
    search_pipeline_runs,
    get_pipeline_run_details,
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
            "Feeds: kalshi, kraken-futures, kraken-spot, polymarket."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "feed": {
                    "type": "string",
                    "description": "Feed name: kalshi, kraken-futures, kraken-spot, or polymarket",
                    "enum": ["kalshi", "kraken-futures", "kraken-spot", "polymarket"],
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
                    "description": "Feed name: kalshi, kraken-futures, kraken-spot, or polymarket",
                    "enum": ["kalshi", "kraken-futures", "kraken-spot", "polymarket"],
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
        name="query_snap",
        description=(
            "Get live ticker snapshots from Redis. Returns the most recent NATS message "
            "per ticker, stored by the snap service with a 5-minute TTL. "
            "Kalshi: yes/no bid/ask/last_price (converted to dollars). "
            "Kraken: bid/ask/last/funding_rate. Polymarket: best_bid/best_ask/spread. "
            "Use without tickers to scan all available snapshots for a feed (max 500)."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "feed": {
                    "type": "string",
                    "description": "Feed name: kalshi, kraken-futures, kraken-spot, or polymarket",
                    "enum": ["kalshi", "kraken-futures", "kraken-spot", "polymarket"],
                },
                "tickers": {
                    "type": "string",
                    "description": "Comma-separated ticker symbols. Omit to scan all.",
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
                    "description": "Optional feed filter: kalshi, kraken-futures, kraken-spot, or polymarket.",
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
                    "enum": ["kalshi", "kraken-futures", "kraken-spot", "polymarket"],
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
        name="browse_categories",
        description=(
            "Browse market categories from the hierarchical monitor index. "
            "Returns category names with event and series counts. "
            "Start here to drill down: categories → series → events → markets."
        ),
        inputSchema={
            "type": "object",
            "properties": {},
        },
    ),
    Tool(
        name="browse_series",
        description=(
            "List all series (product lines) in a category. "
            "E.g., Crypto category has KXBTCD (Bitcoin hourly), KXETHD (Ethereum hourly), etc. "
            "Returns series tickers with titles and counts of active events and markets. "
            "Hierarchy: categories → series → events → markets."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Category to browse (e.g., 'Crypto', 'Economics').",
                },
            },
            "required": ["category"],
        },
    ),
    Tool(
        name="browse_events",
        description=(
            "List all hourly/daily events (contracts) for a series. "
            "Use this to see which contracts exist for a series like KXBTCD. "
            "Returns event tickers (e.g., KXBTCD-26MAR1619), status, strike dates, and market counts. "
            "Kalshi convention: hour in ticker is EST (e.g., 1619 = 4:19 PM EST = 21:19 UTC). "
            "Hierarchy: categories → series → events → markets."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "series": {
                    "type": "string",
                    "description": "Series ticker (e.g., 'KXBTCD' for Bitcoin hourly, 'KXETHD' for Ethereum).",
                },
            },
            "required": ["series"],
        },
    ),
    Tool(
        name="browse_markets",
        description=(
            "Get live prices for all markets (strike levels) in an event/contract. "
            "This is the primary tool for checking if a contract has live data. "
            "Pass an event ticker like KXBTCD-26MAR1619 to see all strike prices "
            "with live bid/ask/last/volume/OI from snap. "
            "Hierarchy: categories → series → events → markets."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "event": {
                    "type": "string",
                    "description": "Event ticker (e.g., 'KXBTCD-26MAR1619' for BTC hourly at 7:19 PM EST).",
                },
            },
            "required": ["event"],
        },
    ),
    Tool(
        name="secmaster_stats",
        description=(
            "Get secmaster database statistics. Returns total counts and status breakdowns "
            "for events, markets, pairs, and conditions across all exchanges. "
            "Requires secmaster:read scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {},
        },
    ),
    Tool(
        name="search_markets",
        description=(
            "Search Kalshi markets from the secmaster database. Filter by category, series, "
            "status, event ticker, or closing time. "
            "Returns market ticker, title, status, close time, volume. "
            "Requires secmaster:read scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Filter by category (e.g., 'Crypto', 'Economics', 'Politics').",
                },
                "series": {
                    "type": "string",
                    "description": "Filter by series ticker (e.g., 'KXBTCD').",
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "closed", "settled"],
                    "description": "Filter by status.",
                },
                "event": {
                    "type": "string",
                    "description": "Filter by parent event ticker.",
                },
                "close_within_hours": {
                    "type": "integer",
                    "description": "Only markets closing within this many hours from now.",
                },
                "closing_after": {
                    "type": "string",
                    "description": "ISO datetime lower bound for close time (e.g., '2026-03-01T00:00:00Z').",
                },
                "as_of": {
                    "type": "string",
                    "description": "ISO date for point-in-time query (e.g., '2026-01-15').",
                },
                "games_only": {
                    "type": "boolean",
                    "description": "Return only game/contest markets.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return. Default 100, max 500.",
                    "default": 100,
                },
            },
        },
    ),
    Tool(
        name="search_events",
        description=(
            "Search Kalshi events from the secmaster database. Filter by category, series, "
            "or status. Returns event ticker, title, category, status, strike date, "
            "and market count. Requires secmaster:read scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Filter by category (e.g., 'Crypto', 'Economics').",
                },
                "series": {
                    "type": "string",
                    "description": "Filter by series ticker.",
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "closed", "settled"],
                    "description": "Filter by status.",
                },
                "as_of": {
                    "type": "string",
                    "description": "ISO date for point-in-time query (e.g., '2026-01-15').",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return. Default 100, max 500.",
                    "default": 100,
                },
            },
        },
    ),
    Tool(
        name="search_pairs",
        description=(
            "Search futures pairs from the secmaster database. Filter by exchange, base currency, "
            "quote currency, market type, or status. "
            "Returns pair ID, symbol, base, quote, market type, status. "
            "Requires secmaster:read scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "exchange": {
                    "type": "string",
                    "description": "Filter by exchange (e.g., 'kraken').",
                },
                "base": {
                    "type": "string",
                    "description": "Filter by base currency (e.g., 'BTC', 'ETH').",
                },
                "quote": {
                    "type": "string",
                    "description": "Filter by quote currency (e.g., 'USD').",
                },
                "market_type": {
                    "type": "string",
                    "enum": ["perpetual", "fixed_maturity"],
                    "description": "Filter by market type.",
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "halted", "delisted"],
                    "description": "Filter by status.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return. Default 100, max 500.",
                    "default": 100,
                },
            },
        },
    ),
    Tool(
        name="search_conditions",
        description=(
            "Search Polymarket conditions from the secmaster database. Filter by category "
            "or status. Returns condition ID, question, status, end date, and token count. "
            "Requires secmaster:read scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Filter by category.",
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "resolved"],
                    "description": "Filter by status.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return. Default 100, max 500.",
                    "default": 100,
                },
            },
        },
    ),
    Tool(
        name="search_lifecycle",
        description=(
            "Search markets by lifecycle status across all exchanges (Kalshi, Kraken, Polymarket). "
            "Filter by status (active/closed/settled), time window (since), and feed. "
            "Returns unified results with exchange, id, title, status, and updatedAt. "
            "Requires secmaster:read scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Filter by market status (e.g., active, closed, settled, suspended).",
                },
                "since": {
                    "type": "string",
                    "description": "ISO datetime lower bound for updated_at (e.g., '2026-02-28T00:00:00Z'). Only returns markets updated after this time.",
                },
                "feed": {
                    "type": "string",
                    "enum": ["kalshi", "kraken-futures", "kraken-spot", "polymarket"],
                    "description": "Optional: filter to a single exchange.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return. Default 100, max 500.",
                    "default": 100,
                },
            },
        },
    ),
    Tool(
        name="get_fees",
        description=(
            "Get fee schedules from the secmaster database. Without a series parameter, "
            "lists current fees for all series (use limit to control count). "
            "With a series parameter, returns the fee schedule for that specific series "
            "(use as_of for historical lookup; limit is ignored). "
            "Requires secmaster:read scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "series": {
                    "type": "string",
                    "description": "Series ticker to get fees for (e.g., 'KXBTCD'). Omit to list all.",
                },
                "as_of": {
                    "type": "string",
                    "description": "ISO date for historical fee schedule. Only used with series.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results when listing all fees. Default 100, max 500.",
                    "default": 100,
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
    Tool(
        name="harman_sessions",
        description=(
            "List all harman OMS sessions with risk/status summary. Returns session ID, "
            "exchange, environment, api_key_prefix, display_name, max_notional, "
            "open_notional, open_order_count, total_fills, total_settlements, "
            "suspended status, and last_activity. Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {},
        },
    ),
    Tool(
        name="harman_orders",
        description=(
            "Query orders for a harman session. Returns order ID, ticker, side, action, "
            "quantity, price, filled_quantity, state, cancel_reason, timestamps. "
            "Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "integer",
                    "description": "Session ID to query orders for.",
                },
                "state": {
                    "type": "string",
                    "description": "Filter by order state (e.g., 'acknowledged', 'filled', 'cancelled').",
                },
                "ticker": {
                    "type": "string",
                    "description": "Filter by ticker (ILIKE pattern match).",
                },
                "since": {
                    "type": "string",
                    "description": "ISO datetime lower bound for created_at.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return. Default 100, max 500.",
                    "default": 100,
                },
                "instance": {
                    "type": "string",
                    "description": "Optional: filter to a specific harman instance (e.g., 'kalshi-demo', 'kalshi-prod').",
                },
            },
            "required": ["session_id"],
        },
    ),
    Tool(
        name="harman_fills",
        description=(
            "Query fills for a harman session. Returns fill ID, order_id, trade_id, "
            "ticker, price, quantity, is_taker, filled_at. Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "integer",
                    "description": "Session ID to query fills for.",
                },
                "ticker": {
                    "type": "string",
                    "description": "Filter by ticker (ILIKE pattern match).",
                },
                "since": {
                    "type": "string",
                    "description": "ISO datetime lower bound for filled_at.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return. Default 100, max 500.",
                    "default": 100,
                },
                "instance": {
                    "type": "string",
                    "description": "Optional: filter to a specific harman instance (e.g., 'kalshi-demo', 'kalshi-prod').",
                },
            },
            "required": ["session_id"],
        },
    ),
    Tool(
        name="harman_order_timeline",
        description=(
            "Get full order lifecycle timeline. Joins prediction_orders + audit_log + "
            "exchange_audit_log + fills + settlements into a unified timeline sorted by "
            "timestamp. The key debugging endpoint for understanding order behavior. "
            "Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "order_id": {
                    "type": "integer",
                    "description": "Order ID to get timeline for.",
                },
                "instance": {
                    "type": "string",
                    "description": "Optional: filter to a specific harman instance (e.g., 'kalshi-demo', 'kalshi-prod').",
                },
            },
            "required": ["order_id"],
        },
    ),
    Tool(
        name="harman_exchange_audit",
        description=(
            "Query exchange audit log for a harman session. Returns REST calls, WS events, "
            "fallback decisions, and reconciliation actions with full request/response payloads. "
            "Categories: rest_call, ws_event, fallback, reconciliation, recovery, risk. "
            "Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "integer",
                    "description": "Session ID to query exchange audit for.",
                },
                "category": {
                    "type": "string",
                    "description": "Filter by category (e.g., 'rest_call', 'ws_event', 'fallback').",
                },
                "action": {
                    "type": "string",
                    "description": "Filter by action (e.g., 'submit_order', 'cancel_order').",
                },
                "outcome": {
                    "type": "string",
                    "description": "Filter by outcome (e.g., 'success', 'error', 'not_found').",
                },
                "since": {
                    "type": "string",
                    "description": "ISO datetime lower bound for created_at.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return. Default 100, max 500.",
                    "default": 100,
                },
                "instance": {
                    "type": "string",
                    "description": "Optional: filter to a specific harman instance (e.g., 'kalshi-demo', 'kalshi-prod').",
                },
            },
            "required": ["session_id"],
        },
    ),
    Tool(
        name="harman_settlements",
        description=(
            "Query settlements for a harman session. Returns settlement ID, ticker, "
            "result, payout_dollars, created_at. Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "integer",
                    "description": "Session ID to query settlements for.",
                },
                "ticker": {
                    "type": "string",
                    "description": "Filter by ticker (ILIKE pattern match).",
                },
                "since": {
                    "type": "string",
                    "description": "ISO datetime lower bound for created_at.",
                },
                "instance": {
                    "type": "string",
                    "description": "Optional: filter to a specific harman instance (e.g., 'kalshi-demo', 'kalshi-prod').",
                },
            },
            "required": ["session_id"],
        },
    ),
    Tool(
        name="list_pipelines",
        description=(
            "List all pipeline definitions with last run status. Returns pipeline ID, "
            "name, description, trigger type, schedule, enabled status, and last run result. "
            "Use to find pipeline IDs for searching runs. Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {},
        },
    ),
    Tool(
        name="search_pipeline_runs",
        description=(
            "Search pipeline runs for a specific pipeline. Returns run ID, status "
            "(pending/running/completed/failed), trigger info, start/finish times, and date. "
            "Use list_pipelines first to find the pipeline ID. Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "pipeline_id": {
                    "type": "integer",
                    "description": "Pipeline ID to search runs for (use list_pipelines to find IDs).",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max runs to return. Default 20.",
                    "default": 20,
                },
            },
            "required": ["pipeline_id"],
        },
    ),
    Tool(
        name="get_pipeline_run_details",
        description=(
            "Get full details of a pipeline run including all stage results. Returns "
            "run metadata plus each stage's status, input config, output data, error message, "
            "and timing. The key debugging tool for pipeline failures. Requires admin scope."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "run_id": {
                    "type": "integer",
                    "description": "Run ID to get details for (from search_pipeline_runs).",
                },
            },
            "required": ["run_id"],
        },
    ),
    Tool(
        name="query_market_lifecycle",
        description=(
            "Query market lifecycle events for a specific market ticker or event. "
            "Returns the full lifecycle history: created, activated, deactivated, "
            "close_date_updated, closed, determined, settled. "
            "Use to audit game/market state transitions."
        ),
        inputSchema={
            "type": "object",
            "properties": {
                "ticker": {
                    "type": "string",
                    "description": "Market ticker (e.g., KXNBAGAME-26MAR05BOSLAL-BOS).",
                },
                "event_ticker": {
                    "type": "string",
                    "description": "Event ticker to get lifecycle for all markets in the event.",
                },
                "since": {
                    "type": "string",
                    "description": "ISO 8601 timestamp to filter events after.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default 50, max 500).",
                    "default": 50,
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
    elif name == "query_snap":
        return query_snap(
            cfg,
            feed=arguments["feed"],
            tickers=arguments.get("tickers"),
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
    elif name == "browse_categories":
        return browse_categories(cfg)
    elif name == "browse_series":
        return browse_series(cfg, category=arguments["category"])
    elif name == "browse_events":
        return browse_events(cfg, series=arguments["series"])
    elif name == "browse_markets":
        return browse_markets(cfg, event=arguments["event"])
    elif name == "secmaster_stats":
        return secmaster_stats(cfg)
    elif name == "search_markets":
        return search_markets(
            cfg,
            category=arguments.get("category"),
            series=arguments.get("series"),
            status=arguments.get("status"),
            event=arguments.get("event"),
            close_within_hours=arguments.get("close_within_hours"),
            closing_after=arguments.get("closing_after"),
            as_of=arguments.get("as_of"),
            games_only=arguments.get("games_only"),
            limit=arguments.get("limit"),
        )
    elif name == "search_events":
        return search_events(
            cfg,
            category=arguments.get("category"),
            series=arguments.get("series"),
            status=arguments.get("status"),
            as_of=arguments.get("as_of"),
            limit=arguments.get("limit"),
        )
    elif name == "search_pairs":
        return search_pairs(
            cfg,
            exchange=arguments.get("exchange"),
            base=arguments.get("base"),
            quote=arguments.get("quote"),
            market_type=arguments.get("market_type"),
            status=arguments.get("status"),
            limit=arguments.get("limit"),
        )
    elif name == "search_conditions":
        return search_conditions(
            cfg,
            category=arguments.get("category"),
            status=arguments.get("status"),
            limit=arguments.get("limit"),
        )
    elif name == "search_lifecycle":
        return search_lifecycle(
            cfg,
            status=arguments.get("status"),
            since=arguments.get("since"),
            feed=arguments.get("feed"),
            limit=arguments.get("limit"),
        )
    elif name == "get_fees":
        return get_fees(
            cfg,
            series=arguments.get("series"),
            as_of=arguments.get("as_of"),
            limit=arguments.get("limit"),
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
    elif name == "harman_sessions":
        return harman_sessions(cfg)
    elif name == "harman_orders":
        return harman_orders(
            cfg,
            session_id=arguments["session_id"],
            state=arguments.get("state"),
            ticker=arguments.get("ticker"),
            since=arguments.get("since"),
            limit=arguments.get("limit", 100),
            instance=arguments.get("instance"),
        )
    elif name == "harman_fills":
        return harman_fills(
            cfg,
            session_id=arguments["session_id"],
            ticker=arguments.get("ticker"),
            since=arguments.get("since"),
            limit=arguments.get("limit", 100),
            instance=arguments.get("instance"),
        )
    elif name == "harman_order_timeline":
        return harman_order_timeline(
            cfg,
            order_id=arguments["order_id"],
            instance=arguments.get("instance"),
        )
    elif name == "harman_exchange_audit":
        return harman_exchange_audit(
            cfg,
            session_id=arguments["session_id"],
            category=arguments.get("category"),
            action=arguments.get("action"),
            outcome=arguments.get("outcome"),
            since=arguments.get("since"),
            limit=arguments.get("limit", 100),
            instance=arguments.get("instance"),
        )
    elif name == "harman_settlements":
        return harman_settlements(
            cfg,
            session_id=arguments["session_id"],
            ticker=arguments.get("ticker"),
            since=arguments.get("since"),
            instance=arguments.get("instance"),
        )
    elif name == "list_pipelines":
        return list_pipelines(cfg)
    elif name == "search_pipeline_runs":
        return search_pipeline_runs(
            cfg,
            pipeline_id=arguments["pipeline_id"],
            limit=arguments.get("limit", 20),
        )
    elif name == "get_pipeline_run_details":
        return get_pipeline_run_details(
            cfg,
            run_id=arguments["run_id"],
        )
    elif name == "query_market_lifecycle":
        return query_market_lifecycle(
            cfg,
            ticker=arguments.get("ticker"),
            event_ticker=arguments.get("event_ticker"),
            since=arguments.get("since"),
            limit=arguments.get("limit", 50),
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
