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


def query_events(cfg: Config, feed: str, date_str: str | None = None, limit: int = 20) -> str:
    """Query event-level trade summaries via ssmd-data-ts API."""
    params: dict[str, Any] = {"feed": feed}
    if date_str:
        params["date"] = date_str
    if limit != 20:
        params["limit"] = limit
    result = api_get(cfg, "/v1/data/events", params)
    return json.dumps(result, default=str)


def query_volume(cfg: Config, date_str: str | None = None, feed: str | None = None) -> str:
    """Query volume summary via ssmd-data-ts API."""
    params: dict[str, Any] = {}
    if date_str:
        params["date"] = date_str
    if feed:
        params["feed"] = feed
    result = api_get(cfg, "/v1/data/volume", params)
    return json.dumps(result, default=str)


# --- Admin tools ---


def list_api_keys(cfg: Config, include_revoked: bool = False) -> str:
    """List all API keys with metadata via ssmd-data-ts API."""
    result = api_get(cfg, "/v1/keys")
    if "error" in result:
        return json.dumps(result, default=str)
    keys = result.get("keys", [])
    if not include_revoked:
        keys = [k for k in keys if not k.get("revokedAt")]
    return json.dumps({"count": len(keys), "keys": keys}, default=str)


def query_key_usage(cfg: Config, key_prefix: str | None = None) -> str:
    """Query API key usage stats (rate limits + token usage + request counts)."""
    # Fetch rate limit / token usage from Redis
    usage_result = api_get(cfg, "/v1/keys/usage")
    usage = usage_result.get("usage", []) if "error" not in usage_result else []

    # Fetch per-key request counts from in-memory Prometheus counter
    requests_result = api_get(cfg, "/v1/keys/requests")
    requests_by_key: dict[str, Any] = {}
    if "error" not in requests_result:
        for k in requests_result.get("keys", []):
            requests_by_key[k["keyPrefix"]] = k

    # Merge: attach request counts to usage entries
    merged = []
    seen_prefixes: set[str] = set()
    for u in usage:
        prefix = u.get("keyPrefix", "")
        seen_prefixes.add(prefix)
        req_data = requests_by_key.get(prefix, {})
        merged.append({
            **u,
            "totalRequests": req_data.get("totalRequests", 0),
            "endpoints": req_data.get("endpoints", []),
        })

    # Add keys that have requests but aren't in usage (e.g., no rate limit data)
    for prefix, req_data in requests_by_key.items():
        if prefix not in seen_prefixes:
            merged.append({
                "keyPrefix": prefix,
                "totalRequests": req_data.get("totalRequests", 0),
                "endpoints": req_data.get("endpoints", []),
            })

    if key_prefix:
        merged = [m for m in merged if m.get("keyPrefix") == key_prefix]

    return json.dumps({"count": len(merged), "sincePodStart": True, "usage": merged}, default=str)
