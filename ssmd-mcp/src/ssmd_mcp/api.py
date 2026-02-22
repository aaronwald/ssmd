"""ssmd-data-ts API client."""

import logging
from typing import Any

import httpx

from ssmd_mcp.config import Config

logger = logging.getLogger(__name__)

# In-memory market cache for session
_market_cache: dict[str, dict[str, Any]] = {}


def _get_client(cfg: Config) -> httpx.Client:
    """Create an httpx client for ssmd-data-ts."""
    headers = {
        "User-Agent": "ssmd-mcp/0.1.0",
        "X-Client-Type": "mcp",
    }
    if cfg.api_key:
        headers["Authorization"] = f"Bearer {cfg.api_key}"
    return httpx.Client(
        base_url=cfg.api_url,
        headers=headers,
        timeout=30.0,
    )


def api_get(cfg: Config, path: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
    """Generic GET request to ssmd-data-ts API."""
    if not cfg.api_url:
        return {"error": "SSMD_API_URL not configured"}
    try:
        with _get_client(cfg) as client:
            resp = client.get(path, params=params)
            resp.raise_for_status()
            return resp.json()
    except httpx.HTTPError as e:
        logger.error("API GET %s failed: %s", path, e)
        return {"error": "API request failed"}


def api_post(cfg: Config, path: str, json_body: dict[str, Any] | None = None) -> dict[str, Any]:
    """Generic POST request to ssmd-data-ts API."""
    if not cfg.api_url:
        return {"error": "SSMD_API_URL not configured"}
    try:
        with _get_client(cfg) as client:
            resp = client.post(path, json=json_body)
            resp.raise_for_status()
            return resp.json()
    except httpx.HTTPError as e:
        logger.error("API POST %s failed: %s", path, e)
        return {"error": "API request failed"}


def lookup_markets(cfg: Config, ids: list[str], feed: str | None = None) -> list[dict[str, Any]]:
    """Look up markets by ID via ssmd-data-ts GET /v1/markets.

    Results are cached in-memory for the session.
    """
    if not cfg.api_url:
        return [{"error": "SSMD_API_URL not configured"}]

    results = []
    uncached_ids = []

    for mid in ids:
        cache_key = f"{feed or ''}:{mid}"
        if cache_key in _market_cache:
            results.append(_market_cache[cache_key])
        else:
            uncached_ids.append(mid)

    if uncached_ids:
        try:
            with _get_client(cfg) as client:
                params: dict[str, Any] = {"ids": ",".join(uncached_ids)}
                if feed:
                    params["feed"] = feed
                resp = client.get("/v1/markets/lookup", params=params)
                resp.raise_for_status()
                data = resp.json()
                markets = data if isinstance(data, list) else data.get("markets", [])
                for m in markets:
                    # Cache by various ID fields
                    mid = m.get("id") or m.get("market_ticker") or m.get("product_id", "")
                    cache_key = f"{feed or ''}:{mid}"
                    _market_cache[cache_key] = m
                    results.append(m)
        except httpx.HTTPError as e:
            logger.error("Market lookup failed: %s", e)
            results.append({"error": "API request failed"})

    return results


def get_catalog(cfg: Config) -> dict[str, Any]:
    """Get feed catalog from ssmd-data-ts."""
    if not cfg.api_url:
        return {"error": "SSMD_API_URL not configured"}
    try:
        with _get_client(cfg) as client:
            resp = client.get("/v1/catalog")
            resp.raise_for_status()
            return resp.json()
    except httpx.HTTPError as e:
        logger.error("Catalog request failed: %s", e)
        return {"error": "API request failed"}
