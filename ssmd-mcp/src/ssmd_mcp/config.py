"""Configuration for ssmd-mcp server."""

import os
from dataclasses import dataclass, field
from dotenv import load_dotenv


@dataclass
class Config:
    api_url: str = ""
    api_key: str = ""
    gcs_bucket: str = "ssmd-data"

    # GCS path mappings (feed_name → gcs_prefix)
    feed_paths: dict[str, str] = field(default_factory=lambda: {
        "kalshi": "kalshi/kalshi/crypto",
        "kraken-futures": "kraken-futures/kraken-futures/futures",
        "polymarket": "polymarket/polymarket/markets",
    })

    # Parquet file types per feed
    feed_types: dict[str, list[str]] = field(default_factory=lambda: {
        "kalshi": ["trade", "ticker"],
        "kraken-futures": ["trade", "ticker"],
        "polymarket": ["best_bid_ask", "last_trade_price", "price_change", "book"],
    })

    # Ticker column for trades per feed
    trade_ticker_col: dict[str, str] = field(default_factory=lambda: {
        "kalshi": "market_ticker",
        "kraken-futures": "product_id",
        "polymarket": "asset_id",
    })

    # Price column for trades per feed
    trade_price_col: dict[str, str] = field(default_factory=lambda: {
        "kalshi": "price",
        "kraken-futures": "price",
        "polymarket": "price",
    })

    # Volume/qty column for trades per feed
    trade_qty_col: dict[str, str] = field(default_factory=lambda: {
        "kalshi": "count",
        "kraken-futures": "qty",
        "polymarket": None,  # last_trade_price has no qty
    })

    # Ticker/price snapshot type per feed
    price_type: dict[str, str] = field(default_factory=lambda: {
        "kalshi": "ticker",
        "kraken-futures": "ticker",
        "polymarket": "best_bid_ask",
    })


def load_config() -> Config:
    load_dotenv()
    return Config(
        api_url=os.getenv("SSMD_API_URL", ""),
        api_key=os.getenv("SSMD_API_KEY", ""),
        gcs_bucket=os.getenv("SSMD_GCS_BUCKET", "ssmd-data"),
    )
