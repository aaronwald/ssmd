"""Tests for ssmd-mcp tool functions.

Uses respx to mock httpx calls to the ssmd-data-ts API.
Each test:
  1. Registers a mock response for the expected API URL
  2. Creates a Config with test values
  3. Calls the tool function
  4. Asserts the response is valid JSON with expected structure
"""

import json

import httpx
import pytest
import respx

from ssmd_mcp.config import Config
from ssmd_mcp import api as api_module
from ssmd_mcp.tools import (
    check_freshness,
    get_fees,
    list_api_keys,
    list_feeds,
    lookup_market,
    query_events,
    query_key_usage,
    query_prices,
    query_trades,
    query_volume,
    search_conditions,
    search_events,
    search_markets,
    search_pairs,
    secmaster_stats,
)

TEST_API_URL = "http://test-api:8080"
TEST_API_KEY = "sk_test_abc123_secret"


@pytest.fixture
def cfg():
    return Config(api_url=TEST_API_URL, api_key=TEST_API_KEY)


@pytest.fixture
def cfg_no_url():
    return Config(api_url="", api_key=TEST_API_KEY)


@pytest.fixture(autouse=True)
def _clear_market_cache():
    """Clear the in-memory market cache between tests."""
    api_module._market_cache.clear()
    yield
    api_module._market_cache.clear()


# ---------------------------------------------------------------------------
# query_trades
# ---------------------------------------------------------------------------


@respx.mock
def test_query_trades(cfg):
    mock_response = {
        "feed": "kalshi",
        "date": "2026-01-01",
        "count": 2,
        "trades": [{"ticker": "BTC-YES", "trade_count": 100, "volume": 50000}],
    }
    respx.get(f"{TEST_API_URL}/v1/data/trades", params={"feed": "kalshi"}).mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = query_trades(cfg, "kalshi")
    parsed = json.loads(result)

    assert parsed["feed"] == "kalshi"
    assert parsed["date"] == "2026-01-01"
    assert parsed["count"] == 2
    assert len(parsed["trades"]) == 1
    assert parsed["trades"][0]["ticker"] == "BTC-YES"
    assert parsed["trades"][0]["volume"] == 50000


@respx.mock
def test_query_trades_with_date_and_limit(cfg):
    mock_response = {"feed": "kraken-futures", "date": "2026-02-15", "count": 0, "trades": []}
    respx.get(
        f"{TEST_API_URL}/v1/data/trades",
        params={"feed": "kraken-futures", "date": "2026-02-15", "limit": 5},
    ).mock(return_value=httpx.Response(200, json=mock_response))

    result = query_trades(cfg, "kraken-futures", date_str="2026-02-15", limit=5)
    parsed = json.loads(result)

    assert parsed["feed"] == "kraken-futures"
    assert parsed["date"] == "2026-02-15"


# ---------------------------------------------------------------------------
# query_prices
# ---------------------------------------------------------------------------


@respx.mock
def test_query_prices(cfg):
    mock_response = {
        "feed": "kalshi",
        "date": "2026-01-01",
        "count": 1,
        "prices": [{"ticker": "BTC-YES", "yes_bid": 0.65, "yes_ask": 0.67}],
    }
    respx.get(f"{TEST_API_URL}/v1/data/prices", params={"feed": "kalshi"}).mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = query_prices(cfg, "kalshi")
    parsed = json.loads(result)

    assert parsed["feed"] == "kalshi"
    assert len(parsed["prices"]) == 1
    assert parsed["prices"][0]["ticker"] == "BTC-YES"


@respx.mock
def test_query_prices_with_hour(cfg):
    mock_response = {"feed": "kalshi", "date": "2026-01-01", "count": 0, "prices": []}
    respx.get(
        f"{TEST_API_URL}/v1/data/prices",
        params={"feed": "kalshi", "hour": "1400"},
    ).mock(return_value=httpx.Response(200, json=mock_response))

    result = query_prices(cfg, "kalshi", hour="1400")
    parsed = json.loads(result)

    assert parsed["feed"] == "kalshi"


# ---------------------------------------------------------------------------
# query_events
# ---------------------------------------------------------------------------


@respx.mock
def test_query_events(cfg):
    mock_response = {
        "feed": "kalshi",
        "date": "2026-01-01",
        "count": 1,
        "events": [
            {
                "event": "KXBTCD-26JAN01",
                "name": "Bitcoin Daily",
                "market_count": 5,
                "total_volume": 12000,
            }
        ],
    }
    respx.get(f"{TEST_API_URL}/v1/data/events", params={"feed": "kalshi"}).mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = query_events(cfg, "kalshi")
    parsed = json.loads(result)

    assert parsed["feed"] == "kalshi"
    assert parsed["count"] == 1
    assert parsed["events"][0]["event"] == "KXBTCD-26JAN01"


# ---------------------------------------------------------------------------
# query_volume
# ---------------------------------------------------------------------------


@respx.mock
def test_query_volume(cfg):
    mock_response = {
        "date": "2026-01-01",
        "feeds": [
            {"feed": "kalshi", "trade_count": 500, "volume": 100000, "ticker_count": 50},
            {"feed": "polymarket", "trade_count": 200, "volume": 50000, "ticker_count": 30},
        ],
    }
    respx.get(f"{TEST_API_URL}/v1/data/volume").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = query_volume(cfg)
    parsed = json.loads(result)

    assert parsed["date"] == "2026-01-01"
    assert len(parsed["feeds"]) == 2


@respx.mock
def test_query_volume_with_feed_filter(cfg):
    mock_response = {
        "date": "2026-01-01",
        "feeds": [{"feed": "kalshi", "trade_count": 500, "volume": 100000, "ticker_count": 50}],
    }
    respx.get(f"{TEST_API_URL}/v1/data/volume", params={"feed": "kalshi"}).mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = query_volume(cfg, feed="kalshi")
    parsed = json.loads(result)

    assert len(parsed["feeds"]) == 1
    assert parsed["feeds"][0]["feed"] == "kalshi"


# ---------------------------------------------------------------------------
# lookup_market
# ---------------------------------------------------------------------------


@respx.mock
def test_lookup_market(cfg):
    mock_response = {
        "markets": [
            {
                "id": "TICKER1",
                "market_ticker": "TICKER1",
                "name": "Test Market",
                "status": "active",
            }
        ]
    }
    respx.get(f"{TEST_API_URL}/v1/markets/lookup", params={"ids": "TICKER1"}).mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = lookup_market(cfg, ["TICKER1"])
    parsed = json.loads(result)

    assert parsed["count"] == 1
    assert parsed["markets"][0]["market_ticker"] == "TICKER1"
    assert parsed["markets"][0]["name"] == "Test Market"


@respx.mock
def test_lookup_market_with_feed(cfg):
    mock_response = {
        "markets": [{"id": "TICKER1", "market_ticker": "TICKER1", "name": "Test"}]
    }
    respx.get(
        f"{TEST_API_URL}/v1/markets/lookup",
        params={"ids": "TICKER1", "feed": "kalshi"},
    ).mock(return_value=httpx.Response(200, json=mock_response))

    result = lookup_market(cfg, ["TICKER1"], feed="kalshi")
    parsed = json.loads(result)

    assert parsed["count"] == 1


@respx.mock
def test_lookup_market_uses_cache(cfg):
    """Second call for same ID should use cache, not make another HTTP request."""
    mock_response = {
        "markets": [{"id": "CACHED1", "market_ticker": "CACHED1", "name": "Cached"}]
    }
    route = respx.get(f"{TEST_API_URL}/v1/markets/lookup", params={"ids": "CACHED1"}).mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    # First call -- hits API
    lookup_market(cfg, ["CACHED1"])
    assert route.call_count == 1

    # Second call -- should use cache
    result = lookup_market(cfg, ["CACHED1"])
    parsed = json.loads(result)
    assert route.call_count == 1  # no additional request
    assert parsed["count"] == 1
    assert parsed["markets"][0]["market_ticker"] == "CACHED1"


# ---------------------------------------------------------------------------
# list_feeds
# ---------------------------------------------------------------------------


@respx.mock
def test_list_feeds(cfg):
    mock_response = {
        "feeds": [
            {"name": "kalshi", "types": ["trades", "ticker"]},
            {"name": "kraken-futures", "types": ["trades", "ticker"]},
            {"name": "polymarket", "types": ["trades", "ticker"]},
        ]
    }
    respx.get(f"{TEST_API_URL}/v1/data/feeds").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = list_feeds(cfg)
    parsed = json.loads(result)

    assert len(parsed["feeds"]) == 3
    assert parsed["feeds"][0]["name"] == "kalshi"


# ---------------------------------------------------------------------------
# check_freshness
# ---------------------------------------------------------------------------


@respx.mock
def test_check_freshness(cfg):
    mock_response = {
        "feeds": [
            {"feed": "kalshi", "newest_file": "2026-01-01T12:00:00Z", "age_hours": 1.5, "stale": False},
        ]
    }
    respx.get(f"{TEST_API_URL}/v1/data/freshness").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = check_freshness(cfg)
    parsed = json.loads(result)

    assert len(parsed["feeds"]) == 1
    assert parsed["feeds"][0]["feed"] == "kalshi"
    assert parsed["feeds"][0]["stale"] is False


@respx.mock
def test_check_freshness_specific_feed(cfg):
    mock_response = {
        "feeds": [{"feed": "kalshi", "newest_file": "2026-01-01T12:00:00Z", "age_hours": 1.0, "stale": False}]
    }
    respx.get(f"{TEST_API_URL}/v1/data/freshness", params={"feed": "kalshi"}).mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = check_freshness(cfg, feed="kalshi")
    parsed = json.loads(result)

    assert parsed["feeds"][0]["feed"] == "kalshi"


# ---------------------------------------------------------------------------
# secmaster_stats
# ---------------------------------------------------------------------------


@respx.mock
def test_secmaster_stats(cfg):
    mock_response = {
        "events": {"total": 100, "active": 80, "closed": 20},
        "markets": {"total": 500, "active": 400},
        "pairs": {"total": 50, "active": 45},
        "conditions": {"total": 200, "active": 150},
    }
    respx.get(f"{TEST_API_URL}/v1/secmaster/stats").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = secmaster_stats(cfg)
    parsed = json.loads(result)

    assert parsed["events"]["total"] == 100
    assert parsed["markets"]["total"] == 500
    assert parsed["pairs"]["total"] == 50
    assert parsed["conditions"]["total"] == 200


# ---------------------------------------------------------------------------
# search_markets
# ---------------------------------------------------------------------------


@respx.mock
def test_search_markets(cfg):
    mock_response = {
        "count": 1,
        "markets": [{"ticker": "KXBTCD-26JAN01-T100000", "title": "BTC > $100k", "status": "active"}],
    }
    respx.get(f"{TEST_API_URL}/v1/markets").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = search_markets(cfg)
    parsed = json.loads(result)

    assert parsed["count"] == 1
    assert parsed["markets"][0]["ticker"] == "KXBTCD-26JAN01-T100000"


@respx.mock
def test_search_markets_with_filters(cfg):
    mock_response = {"count": 0, "markets": []}
    respx.get(
        f"{TEST_API_URL}/v1/markets",
        params={"category": "Crypto", "status": "active", "limit": 10},
    ).mock(return_value=httpx.Response(200, json=mock_response))

    result = search_markets(cfg, category="Crypto", status="active", limit=10)
    parsed = json.loads(result)

    assert parsed["count"] == 0


@respx.mock
def test_search_markets_limit_clamped(cfg):
    """Limit above MAX_LIMIT (500) should be clamped."""
    mock_response = {"count": 0, "markets": []}
    respx.get(
        f"{TEST_API_URL}/v1/markets",
        params={"limit": 500},
    ).mock(return_value=httpx.Response(200, json=mock_response))

    result = search_markets(cfg, limit=9999)
    parsed = json.loads(result)

    assert parsed["count"] == 0


# ---------------------------------------------------------------------------
# search_events
# ---------------------------------------------------------------------------


@respx.mock
def test_search_events(cfg):
    mock_response = {
        "count": 1,
        "events": [{"ticker": "KXBTCD-26JAN01", "title": "Bitcoin Daily", "status": "active"}],
    }
    respx.get(f"{TEST_API_URL}/v1/events").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = search_events(cfg)
    parsed = json.loads(result)

    assert parsed["count"] == 1
    assert parsed["events"][0]["ticker"] == "KXBTCD-26JAN01"


@respx.mock
def test_search_events_with_filters(cfg):
    mock_response = {"count": 0, "events": []}
    respx.get(
        f"{TEST_API_URL}/v1/events",
        params={"category": "Crypto", "series": "KXBTCD"},
    ).mock(return_value=httpx.Response(200, json=mock_response))

    result = search_events(cfg, category="Crypto", series="KXBTCD")
    parsed = json.loads(result)

    assert parsed["count"] == 0


# ---------------------------------------------------------------------------
# search_pairs
# ---------------------------------------------------------------------------


@respx.mock
def test_search_pairs(cfg):
    mock_response = {
        "count": 1,
        "pairs": [
            {"pair_id": "PI_XBTUSD", "base": "BTC", "quote": "USD", "market_type": "perpetual", "status": "active"}
        ],
    }
    respx.get(f"{TEST_API_URL}/v1/pairs").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = search_pairs(cfg)
    parsed = json.loads(result)

    assert parsed["count"] == 1
    assert parsed["pairs"][0]["pair_id"] == "PI_XBTUSD"


@respx.mock
def test_search_pairs_with_filters(cfg):
    mock_response = {"count": 0, "pairs": []}
    respx.get(
        f"{TEST_API_URL}/v1/pairs",
        params={"exchange": "kraken", "base": "BTC", "status": "active"},
    ).mock(return_value=httpx.Response(200, json=mock_response))

    result = search_pairs(cfg, exchange="kraken", base="BTC", status="active")
    parsed = json.loads(result)

    assert parsed["count"] == 0


# ---------------------------------------------------------------------------
# search_conditions
# ---------------------------------------------------------------------------


@respx.mock
def test_search_conditions(cfg):
    mock_response = {
        "count": 1,
        "conditions": [
            {"condition_id": "0xabc123", "question": "Will BTC exceed $100k?", "status": "active"}
        ],
    }
    respx.get(f"{TEST_API_URL}/v1/conditions").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = search_conditions(cfg)
    parsed = json.loads(result)

    assert parsed["count"] == 1
    assert parsed["conditions"][0]["condition_id"] == "0xabc123"


@respx.mock
def test_search_conditions_with_filters(cfg):
    mock_response = {"count": 0, "conditions": []}
    respx.get(
        f"{TEST_API_URL}/v1/conditions",
        params={"category": "Crypto", "status": "active"},
    ).mock(return_value=httpx.Response(200, json=mock_response))

    result = search_conditions(cfg, category="Crypto", status="active")
    parsed = json.loads(result)

    assert parsed["count"] == 0


# ---------------------------------------------------------------------------
# get_fees
# ---------------------------------------------------------------------------


@respx.mock
def test_get_fees_all(cfg):
    mock_response = {
        "count": 2,
        "fees": [
            {"series": "KXBTCD", "taker_fee": 0.07, "maker_rebate": 0.0},
            {"series": "KXETHD", "taker_fee": 0.07, "maker_rebate": 0.0},
        ],
    }
    respx.get(f"{TEST_API_URL}/v1/fees").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = get_fees(cfg)
    parsed = json.loads(result)

    assert parsed["count"] == 2
    assert parsed["fees"][0]["series"] == "KXBTCD"


@respx.mock
def test_get_fees_specific_series(cfg):
    mock_response = {
        "series": "KXBTCD",
        "history": [{"effective_date": "2026-01-01", "taker_fee": 0.07}],
    }
    respx.get(f"{TEST_API_URL}/v1/fees/KXBTCD").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = get_fees(cfg, series="KXBTCD")
    parsed = json.loads(result)

    assert parsed["series"] == "KXBTCD"
    assert len(parsed["history"]) == 1


@respx.mock
def test_get_fees_specific_series_with_as_of(cfg):
    mock_response = {
        "series": "KXBTCD",
        "history": [{"effective_date": "2025-06-01", "taker_fee": 0.05}],
    }
    respx.get(f"{TEST_API_URL}/v1/fees/KXBTCD", params={"as_of": "2025-07-01"}).mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = get_fees(cfg, series="KXBTCD", as_of="2025-07-01")
    parsed = json.loads(result)

    assert parsed["series"] == "KXBTCD"


def test_get_fees_invalid_series(cfg):
    """Invalid series format should return error without making API call."""
    result = get_fees(cfg, series="DROP TABLE; --")
    parsed = json.loads(result)

    assert "error" in parsed
    assert parsed["error"] == "Invalid series format"


# ---------------------------------------------------------------------------
# list_api_keys
# ---------------------------------------------------------------------------


@respx.mock
def test_list_api_keys(cfg):
    mock_response = {
        "keys": [
            {
                "prefix": "sk_abc",
                "name": "test-key",
                "email": "test@example.com",
                "scopes": ["read"],
                "revokedAt": None,
            },
            {
                "prefix": "sk_def",
                "name": "revoked-key",
                "email": "test@example.com",
                "scopes": ["read"],
                "revokedAt": "2026-01-15T00:00:00Z",
            },
        ]
    }
    respx.get(f"{TEST_API_URL}/v1/keys").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = list_api_keys(cfg)
    parsed = json.loads(result)

    # Default excludes revoked keys
    assert parsed["count"] == 1
    assert parsed["keys"][0]["prefix"] == "sk_abc"


@respx.mock
def test_list_api_keys_include_revoked(cfg):
    mock_response = {
        "keys": [
            {"prefix": "sk_abc", "name": "active", "revokedAt": None},
            {"prefix": "sk_def", "name": "revoked", "revokedAt": "2026-01-15T00:00:00Z"},
        ]
    }
    respx.get(f"{TEST_API_URL}/v1/keys").mock(
        return_value=httpx.Response(200, json=mock_response)
    )

    result = list_api_keys(cfg, include_revoked=True)
    parsed = json.loads(result)

    assert parsed["count"] == 2


# ---------------------------------------------------------------------------
# query_key_usage
# ---------------------------------------------------------------------------


@respx.mock
def test_query_key_usage(cfg):
    usage_response = {
        "usage": [
            {
                "keyPrefix": "sk_abc",
                "requestsInWindow": 10,
                "limit": 100,
                "tier": "standard",
                "promptTokens": 5000,
                "completionTokens": 2000,
            }
        ]
    }
    requests_response = {
        "keys": [
            {
                "keyPrefix": "sk_abc",
                "totalRequests": 42,
                "endpoints": [{"path": "/v1/data/trades", "count": 30}],
            }
        ]
    }
    respx.get(f"{TEST_API_URL}/v1/keys/usage").mock(
        return_value=httpx.Response(200, json=usage_response)
    )
    respx.get(f"{TEST_API_URL}/v1/keys/requests").mock(
        return_value=httpx.Response(200, json=requests_response)
    )

    result = query_key_usage(cfg)
    parsed = json.loads(result)

    assert parsed["count"] == 1
    assert parsed["sincePodStart"] is True
    assert parsed["usage"][0]["keyPrefix"] == "sk_abc"
    assert parsed["usage"][0]["totalRequests"] == 42
    assert parsed["usage"][0]["promptTokens"] == 5000
    assert len(parsed["usage"][0]["endpoints"]) == 1


@respx.mock
def test_query_key_usage_with_prefix_filter(cfg):
    usage_response = {
        "usage": [
            {"keyPrefix": "sk_abc", "requestsInWindow": 10},
            {"keyPrefix": "sk_def", "requestsInWindow": 5},
        ]
    }
    requests_response = {
        "keys": [
            {"keyPrefix": "sk_abc", "totalRequests": 42, "endpoints": []},
            {"keyPrefix": "sk_def", "totalRequests": 20, "endpoints": []},
        ]
    }
    respx.get(f"{TEST_API_URL}/v1/keys/usage").mock(
        return_value=httpx.Response(200, json=usage_response)
    )
    respx.get(f"{TEST_API_URL}/v1/keys/requests").mock(
        return_value=httpx.Response(200, json=requests_response)
    )

    result = query_key_usage(cfg, key_prefix="sk_abc")
    parsed = json.loads(result)

    assert parsed["count"] == 1
    assert parsed["usage"][0]["keyPrefix"] == "sk_abc"


@respx.mock
def test_query_key_usage_merges_request_only_keys(cfg):
    """Keys with requests but no usage data should still appear."""
    usage_response = {"usage": []}
    requests_response = {
        "keys": [{"keyPrefix": "sk_new", "totalRequests": 5, "endpoints": []}]
    }
    respx.get(f"{TEST_API_URL}/v1/keys/usage").mock(
        return_value=httpx.Response(200, json=usage_response)
    )
    respx.get(f"{TEST_API_URL}/v1/keys/requests").mock(
        return_value=httpx.Response(200, json=requests_response)
    )

    result = query_key_usage(cfg)
    parsed = json.loads(result)

    assert parsed["count"] == 1
    assert parsed["usage"][0]["keyPrefix"] == "sk_new"
    assert parsed["usage"][0]["totalRequests"] == 5


# ---------------------------------------------------------------------------
# Error handling
# ---------------------------------------------------------------------------


@respx.mock
def test_api_500_returns_error(cfg):
    """API returning 500 should result in error JSON."""
    respx.get(f"{TEST_API_URL}/v1/data/trades", params={"feed": "kalshi"}).mock(
        return_value=httpx.Response(500, json={"error": "Internal Server Error"})
    )

    result = query_trades(cfg, "kalshi")
    parsed = json.loads(result)

    assert "error" in parsed


@respx.mock
def test_api_404_returns_error(cfg):
    """API returning 404 should result in error JSON."""
    respx.get(f"{TEST_API_URL}/v1/data/feeds").mock(
        return_value=httpx.Response(404, json={"error": "Not Found"})
    )

    result = list_feeds(cfg)
    parsed = json.loads(result)

    assert "error" in parsed


def test_api_url_not_configured(cfg_no_url):
    """Empty API URL should return an error without making any request."""
    result = query_trades(cfg_no_url, "kalshi")
    parsed = json.loads(result)

    assert "error" in parsed
    assert "not configured" in parsed["error"]


def test_api_url_not_configured_list_feeds(cfg_no_url):
    result = list_feeds(cfg_no_url)
    parsed = json.loads(result)

    assert "error" in parsed


def test_api_url_not_configured_lookup_market(cfg_no_url):
    result = lookup_market(cfg_no_url, ["TICKER1"])
    parsed = json.loads(result)

    # lookup_market wraps in {count, markets}
    assert parsed["count"] == 1
    assert "error" in parsed["markets"][0]


def test_api_url_not_configured_secmaster_stats(cfg_no_url):
    result = secmaster_stats(cfg_no_url)
    parsed = json.loads(result)

    assert "error" in parsed


def test_api_url_not_configured_query_key_usage(cfg_no_url):
    """query_key_usage makes two API calls; both should fail gracefully."""
    result = query_key_usage(cfg_no_url)
    parsed = json.loads(result)

    # Should still return valid structure with count 0
    assert parsed["count"] == 0
    assert parsed["sincePodStart"] is True


@respx.mock
def test_api_connection_error_returns_error(cfg):
    """Network-level failure should result in error JSON."""
    respx.get(f"{TEST_API_URL}/v1/data/volume").mock(
        side_effect=httpx.ConnectError("Connection refused")
    )

    result = query_volume(cfg)
    parsed = json.loads(result)

    assert "error" in parsed


@respx.mock
def test_api_timeout_returns_error(cfg):
    """Request timeout should result in error JSON."""
    respx.get(f"{TEST_API_URL}/v1/secmaster/stats").mock(
        side_effect=httpx.ReadTimeout("Read timed out")
    )

    result = secmaster_stats(cfg)
    parsed = json.loads(result)

    assert "error" in parsed


# ---------------------------------------------------------------------------
# Auth header verification
# ---------------------------------------------------------------------------


@respx.mock
def test_auth_header_sent(cfg):
    """API calls should include Bearer authorization header."""
    route = respx.get(f"{TEST_API_URL}/v1/data/feeds").mock(
        return_value=httpx.Response(200, json={"feeds": []})
    )

    list_feeds(cfg)

    assert route.call_count == 1
    request = route.calls[0].request
    assert request.headers["authorization"] == f"Bearer {TEST_API_KEY}"
    assert request.headers["user-agent"] == "ssmd-mcp/0.1.0"
    assert request.headers["x-client-type"] == "mcp"


@respx.mock
def test_no_auth_header_when_no_key():
    """When api_key is empty, no Authorization header should be sent."""
    cfg_no_key = Config(api_url=TEST_API_URL, api_key="")
    route = respx.get(f"{TEST_API_URL}/v1/data/feeds").mock(
        return_value=httpx.Response(200, json={"feeds": []})
    )

    list_feeds(cfg_no_key)

    assert route.call_count == 1
    request = route.calls[0].request
    assert "authorization" not in request.headers
