"""MCP tool implementations for ssmd-mcp — pure API client."""

import json
import logging
from typing import Any

from ssmd_mcp.config import Config
from ssmd_mcp.api import api_get, lookup_markets

logger = logging.getLogger(__name__)


def query_trades(cfg: Config, feed: str, date_str: str | None = None, limit: int = 20) -> str:
    """Query trade data via ssmd-data-ts API."""
    params: dict[str, Any] = {"feed": feed}
    if date_str:
        params["date"] = date_str
    if limit != 20:
        params["limit"] = limit
    result = api_get(cfg, "/v1/data/trades", params)
    return json.dumps(result, default=str)


def query_prices(cfg: Config, feed: str, date_str: str | None = None, hour: str | None = None) -> str:
    """Query price snapshots via ssmd-data-ts API."""
    params: dict[str, Any] = {"feed": feed}
    if date_str:
        params["date"] = date_str
    if hour:
        params["hour"] = hour
    result = api_get(cfg, "/v1/data/prices", params)
    return json.dumps(result, default=str)


def lookup_market(cfg: Config, ids: list[str], feed: str | None = None) -> str:
    """Look up market metadata via ssmd-data-ts API."""
    results = lookup_markets(cfg, ids, feed)
    return json.dumps({
        "count": len(results),
        "markets": results,
    }, default=str)


def list_feeds(cfg: Config) -> str:
    """List available feeds via ssmd-data-ts API."""
    result = api_get(cfg, "/v1/data/feeds")
    return json.dumps(result, default=str)


def check_freshness(cfg: Config, feed: str | None = None) -> str:
    """Check data freshness via ssmd-data-ts API."""
    params: dict[str, Any] = {}
    if feed:
        params["feed"] = feed
    result = api_get(cfg, "/v1/data/freshness", params)
    return json.dumps(result, default=str)
