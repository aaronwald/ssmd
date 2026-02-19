"""MCP tool implementations for ssmd-mcp."""

import json
import logging
import os
import re
import tempfile
from datetime import datetime
from typing import Any

import duckdb

from ssmd_mcp.config import Config
from ssmd_mcp.gcs import (
    gcs_parquet_path,
    gcs_gs_path,
    get_connection,
    today_str,
    list_gcs_dates,
    list_gcs_files,
    _gsutil_download,
)
from ssmd_mcp.api import lookup_markets, get_catalog

logger = logging.getLogger(__name__)


def _try_query(conn: duckdb.DuckDBPyConnection, sql: str) -> duckdb.DuckDBPyRelation | None:
    """Execute a query, return relation or None on error."""
    try:
        return conn.execute(sql)
    except duckdb.IOException as e:
        logger.warning("DuckDB GCS query failed: %s", e)
        return None
    except duckdb.CatalogException as e:
        logger.warning("DuckDB catalog error: %s", e)
        return None


def _query_with_fallback(
    conn: duckdb.DuckDBPyConnection,
    cfg: Config,
    feed: str,
    date_str: str,
    file_type: str,
    sql_template: str,
    hour: str | None = None,
) -> list[dict[str, Any]]:
    """Query parquet via DuckDB GCS, falling back to gsutil download.

    sql_template should contain {path} placeholder.
    """
    gcs_path = gcs_parquet_path(cfg, feed, date_str, file_type, hour)

    # Try direct GCS access (path is already a quoted string for read_parquet)
    sql = sql_template.format(path=f"'{gcs_path}'")
    result = _try_query(conn, sql)
    if result is not None:
        cols = [desc[0] for desc in result.description]
        return [dict(zip(cols, row)) for row in result.fetchall()]

    # Fall back: download via gsutil to /tmp
    logger.info("Falling back to gsutil download for %s", gcs_path)
    if hour is None:
        # Glob pattern — list and download matching files
        files = list_gcs_files(cfg, feed, date_str)
        matching = [f for f in files if f.endswith(".parquet") and f"/{file_type}_" in f]
        if not matching:
            return []
        local_paths = []
        for gf in matching:
            fname = gf.rstrip("/").split("/")[-1]
            local = os.path.join(tempfile.gettempdir(), f"ssmd_{feed}_{date_str}_{fname}")
            if not os.path.exists(local):
                _gsutil_download(gf, local)
            if os.path.exists(local):
                local_paths.append(local)
        if not local_paths:
            return []
        # DuckDB list syntax for multiple files: ['path1', 'path2']
        path_expr = "[" + ", ".join(f"'{p}'" for p in local_paths) + "]"
    else:
        fname = f"{file_type}_{hour}.parquet"
        local = os.path.join(tempfile.gettempdir(), f"ssmd_{feed}_{date_str}_{fname}")
        gs_path_gs = gcs_path.replace("s3://", "gs://")
        if not os.path.exists(local):
            _gsutil_download(gs_path_gs, local)
        if not os.path.exists(local):
            return []
        path_expr = f"'{local}'"

    sql = sql_template.format(path=path_expr)
    result = conn.execute(sql)
    cols = [desc[0] for desc in result.description]
    return [dict(zip(cols, row)) for row in result.fetchall()]


def query_trades(cfg: Config, feed: str, date_str: str | None = None, limit: int = 20) -> str:
    """Query trade parquet files: group by ticker, count trades, sum volume, get price range."""
    if feed not in cfg.feed_paths:
        return json.dumps({"error": f"Unknown feed: {feed}. Valid: {list(cfg.feed_paths.keys())}"})

    date_str = date_str or today_str()
    conn = get_connection(cfg)

    ticker_col = cfg.trade_ticker_col[feed]
    price_col = cfg.trade_price_col[feed]
    qty_col = cfg.trade_qty_col.get(feed)

    # Build SQL based on feed
    if feed == "polymarket":
        # Polymarket uses last_trade_price which has asset_id, price
        file_type = "last_trade_price"
        sql_template = f"""
            SELECT
                {ticker_col} as ticker,
                COUNT(*) as trade_count,
                MIN({price_col}) as min_price,
                MAX({price_col}) as max_price,
                AVG({price_col}) as avg_price
            FROM read_parquet({{path}})
            GROUP BY {ticker_col}
            ORDER BY trade_count DESC
            LIMIT {limit}
        """
    elif feed == "kalshi":
        file_type = "trade"
        # Kalshi prices are in cents
        sql_template = f"""
            SELECT
                {ticker_col} as ticker,
                COUNT(*) as trade_count,
                SUM({qty_col}) as total_volume,
                MIN({price_col}) / 100.0 as min_price,
                MAX({price_col}) / 100.0 as max_price,
                AVG({price_col}) / 100.0 as avg_price
            FROM read_parquet({{path}})
            GROUP BY {ticker_col}
            ORDER BY trade_count DESC
            LIMIT {limit}
        """
    else:
        # Kraken
        file_type = "trade"
        sql_template = f"""
            SELECT
                {ticker_col} as ticker,
                COUNT(*) as trade_count,
                SUM({qty_col}) as total_volume,
                MIN({price_col}) as min_price,
                MAX({price_col}) as max_price,
                AVG({price_col}) as avg_price
            FROM read_parquet({{path}})
            GROUP BY {ticker_col}
            ORDER BY trade_count DESC
            LIMIT {limit}
        """

    rows = _query_with_fallback(conn, cfg, feed, date_str, file_type, sql_template)
    conn.close()

    return json.dumps({
        "feed": feed,
        "date": date_str,
        "count": len(rows),
        "trades": rows,
    }, default=str)


def query_prices(cfg: Config, feed: str, date_str: str | None = None, hour: str | None = None) -> str:
    """Query latest price snapshot per instrument."""
    if feed not in cfg.feed_paths:
        return json.dumps({"error": f"Unknown feed: {feed}. Valid: {list(cfg.feed_paths.keys())}"})

    date_str = date_str or today_str()
    file_type = cfg.price_type[feed]
    conn = get_connection(cfg)

    if feed == "kalshi":
        sql_template = """
            SELECT
                market_ticker as ticker,
                yes_bid / 100.0 as yes_bid,
                yes_ask / 100.0 as yes_ask,
                no_bid / 100.0 as no_bid,
                no_ask / 100.0 as no_ask,
                last_price / 100.0 as last_price,
                volume,
                open_interest,
                ts
            FROM read_parquet({path})
            QUALIFY ROW_NUMBER() OVER (PARTITION BY market_ticker ORDER BY ts DESC) = 1
            ORDER BY volume DESC
        """
    elif feed == "kraken-futures":
        sql_template = """
            SELECT
                product_id as ticker,
                bid,
                ask,
                last,
                volume,
                funding_rate,
                mark_price
            FROM read_parquet({path})
            QUALIFY ROW_NUMBER() OVER (PARTITION BY product_id ORDER BY _received_at DESC) = 1
            ORDER BY volume DESC
        """
    else:
        # Polymarket best_bid_ask
        sql_template = """
            SELECT
                market,
                asset_id,
                best_bid,
                best_ask,
                spread
            FROM read_parquet({path})
            QUALIFY ROW_NUMBER() OVER (PARTITION BY asset_id ORDER BY _received_at DESC) = 1
            ORDER BY spread ASC
        """

    rows = _query_with_fallback(conn, cfg, feed, date_str, file_type, sql_template, hour)
    conn.close()

    return json.dumps({
        "feed": feed,
        "date": date_str,
        "hour": hour,
        "count": len(rows),
        "prices": rows,
    }, default=str)


def query_raw(cfg: Config, sql: str, feed: str | None = None, date_str: str | None = None) -> str:
    """Execute freeform DuckDB SQL with $FEED_PATH macro expansion."""
    date_str = date_str or today_str()
    conn = get_connection(cfg)

    # Expand $FEED_PATH(feed, date) macros
    def expand_feed_path(match: re.Match) -> str:
        f = match.group(1)
        d = match.group(2) if match.group(2) else date_str
        prefix = cfg.feed_paths.get(f, f)
        return f"s3://{cfg.gcs_bucket}/{prefix}/{d}/"

    expanded = re.sub(
        r'\$FEED_PATH\(\s*([a-z-]+)\s*(?:,\s*([0-9-]+)\s*)?\)',
        expand_feed_path,
        sql,
    )

    # Safety: enforce row limit
    if "LIMIT" not in expanded.upper():
        expanded = expanded.rstrip("; \n") + " LIMIT 1000"

    try:
        result = conn.execute(expanded)
        cols = [desc[0] for desc in result.description]
        rows = [dict(zip(cols, row)) for row in result.fetchall()]
        conn.close()
        return json.dumps({
            "sql": expanded,
            "count": len(rows),
            "columns": cols,
            "rows": rows,
        }, default=str)
    except Exception as e:
        conn.close()
        return json.dumps({"error": str(e), "sql": expanded})


def lookup_market(cfg: Config, ids: list[str], feed: str | None = None) -> str:
    """Look up market metadata via ssmd-data-ts API."""
    results = lookup_markets(cfg, ids, feed)
    return json.dumps({
        "count": len(results),
        "markets": results,
    }, default=str)


def list_feeds(cfg: Config) -> str:
    """List available feeds with GCS dates and catalog info."""
    feeds_info = []

    # Get catalog from API if available
    catalog = get_catalog(cfg) if cfg.api_url else {}

    for feed_name, gcs_prefix in cfg.feed_paths.items():
        dates = list_gcs_dates(cfg, feed_name, max_dates=10)
        feed_info: dict[str, Any] = {
            "feed": feed_name,
            "gcs_prefix": gcs_prefix,
            "parquet_types": cfg.feed_types.get(feed_name, []),
            "available_dates": dates,
            "date_count": len(dates),
        }
        # Merge catalog info if available
        if isinstance(catalog, dict) and "feeds" in catalog:
            for cf in catalog["feeds"]:
                if cf.get("name") == feed_name or cf.get("feed") == feed_name:
                    feed_info["catalog"] = cf
                    break
        feeds_info.append(feed_info)

    return json.dumps({"feeds": feeds_info}, default=str)


def check_freshness(cfg: Config, feed: str | None = None) -> str:
    """Check data freshness: find newest files per feed, report age, flag stale."""
    feeds = [feed] if feed else list(cfg.feed_paths.keys())
    results = []
    stale_threshold_hours = 7

    for f in feeds:
        dates = list_gcs_dates(cfg, f, max_dates=3)
        if not dates:
            results.append({
                "feed": f,
                "status": "no_data",
                "newest_date": None,
                "stale": True,
            })
            continue

        newest_date = dates[0]
        files = list_gcs_files(cfg, f, newest_date)

        # Find newest file by name (hour in filename)
        newest_hour = None
        parquet_files = [fp for fp in files if fp.endswith(".parquet")]
        jsonl_files = [fp for fp in files if fp.endswith(".jsonl") or fp.endswith(".jsonl.gz")]
        all_data_files = parquet_files + jsonl_files

        for fp in all_data_files:
            fname = fp.rstrip("/").split("/")[-1]
            # Extract hour from filename like trade_1400.parquet
            parts = fname.replace(".parquet", "").replace(".jsonl.gz", "").replace(".jsonl", "").split("_")
            if len(parts) >= 2:
                hour_part = parts[-1]
                if hour_part.isdigit() and len(hour_part) == 4:
                    if newest_hour is None or hour_part > newest_hour:
                        newest_hour = hour_part

        # Calculate age
        now = datetime.utcnow()
        try:
            date_obj = datetime.strptime(newest_date, "%Y-%m-%d")
            if newest_hour:
                h = int(newest_hour[:2])
                date_obj = date_obj.replace(hour=h)
            age_hours = (now - date_obj).total_seconds() / 3600
        except ValueError:
            age_hours = None

        is_stale = age_hours is not None and age_hours > stale_threshold_hours

        results.append({
            "feed": f,
            "status": "stale" if is_stale else "fresh",
            "newest_date": newest_date,
            "newest_hour": newest_hour,
            "age_hours": round(age_hours, 1) if age_hours is not None else None,
            "stale": is_stale,
            "file_count": len(all_data_files),
            "parquet_count": len(parquet_files),
        })

    return json.dumps({
        "checked_at": datetime.utcnow().isoformat() + "Z",
        "stale_threshold_hours": stale_threshold_hours,
        "feeds": results,
    }, default=str)
