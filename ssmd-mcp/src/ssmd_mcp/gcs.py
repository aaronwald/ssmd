"""GCS + DuckDB data access for parquet files."""

import subprocess
import logging
from datetime import date, datetime

import duckdb

from ssmd_mcp.config import Config

logger = logging.getLogger(__name__)


def _setup_duckdb_gcs(conn: duckdb.DuckDBPyConnection) -> bool:
    """Configure DuckDB for GCS access. Returns True if successful."""
    try:
        conn.execute("INSTALL httpfs; LOAD httpfs;")
    except duckdb.IOException:
        # Already installed
        conn.execute("LOAD httpfs;")

    conn.execute("SET s3_endpoint='storage.googleapis.com';")
    conn.execute("SET s3_url_style='path';")

    # Use GCS credential chain (requires `gcloud auth application-default login`)
    try:
        conn.execute("CREATE SECRET IF NOT EXISTS gcs_secret (TYPE GCS, PROVIDER CREDENTIAL_CHAIN);")
        return True
    except Exception as e:
        logger.warning("GCS credential chain failed: %s. Will fall back to gsutil.", e)

    return False


def _gsutil_download(gcs_path: str, local_path: str) -> bool:
    """Download a GCS file via gsutil as fallback."""
    try:
        result = subprocess.run(
            ["gsutil", "cp", gcs_path, local_path],
            capture_output=True, text=True, timeout=60,
        )
        return result.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return False


def _gsutil_ls(gcs_path: str) -> list[str]:
    """List GCS objects via gsutil."""
    try:
        result = subprocess.run(
            ["gsutil", "ls", gcs_path],
            capture_output=True, text=True, timeout=30,
        )
        if result.returncode == 0:
            return [line.strip() for line in result.stdout.strip().split("\n") if line.strip()]
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    return []


def gcs_parquet_path(cfg: Config, feed: str, date_str: str, file_type: str, hour: str | None = None) -> str:
    """Build the GCS path for parquet files.

    Returns a glob path if hour is None, or a specific file path.
    """
    prefix = cfg.feed_paths.get(feed, feed)
    bucket = cfg.gcs_bucket
    if hour is not None:
        return f"s3://{bucket}/{prefix}/{date_str}/{file_type}_{hour}.parquet"
    return f"s3://{bucket}/{prefix}/{date_str}/{file_type}_*.parquet"


def gcs_gs_path(cfg: Config, feed: str, date_str: str | None = None) -> str:
    """Build gs:// path for gsutil operations."""
    prefix = cfg.feed_paths.get(feed, feed)
    bucket = cfg.gcs_bucket
    if date_str:
        return f"gs://{bucket}/{prefix}/{date_str}/"
    return f"gs://{bucket}/{prefix}/"


def get_connection(cfg: Config) -> duckdb.DuckDBPyConnection:
    """Create a DuckDB connection with GCS configured."""
    conn = duckdb.connect()
    success = _setup_duckdb_gcs(conn)
    if not success:
        logger.warning("GCS direct access not configured; will fall back to gsutil downloads")
    return conn


def today_str() -> str:
    return date.today().strftime("%Y-%m-%d")


def list_gcs_dates(cfg: Config, feed: str, max_dates: int = 30) -> list[str]:
    """List available dates for a feed in GCS."""
    gs_path = gcs_gs_path(cfg, feed)
    entries = _gsutil_ls(gs_path)
    dates = []
    for entry in entries:
        # entries look like gs://bucket/prefix/2025-01-15/
        parts = entry.rstrip("/").split("/")
        if parts:
            candidate = parts[-1]
            # Simple date format check
            if len(candidate) == 10 and candidate[4] == "-" and candidate[7] == "-":
                dates.append(candidate)
    dates.sort(reverse=True)
    return dates[:max_dates]


def list_gcs_files(cfg: Config, feed: str, date_str: str) -> list[str]:
    """List files in a GCS date directory."""
    gs_path = gcs_gs_path(cfg, feed, date_str)
    return _gsutil_ls(gs_path)
