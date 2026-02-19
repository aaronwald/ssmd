"""Configuration for ssmd-mcp server."""

import os
from dataclasses import dataclass

from dotenv import load_dotenv


@dataclass
class Config:
    api_url: str = ""
    api_key: str = ""


def load_config() -> Config:
    load_dotenv()
    return Config(
        api_url=os.getenv("SSMD_API_URL", ""),
        api_key=os.getenv("SSMD_API_KEY", ""),
    )
