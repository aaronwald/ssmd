"""GCS + DuckDB data access for parquet files."""

import subprocess
import logging
from datetime import date, datetime

import duckdb

from ssmd_mcp.config import Config

logger = logging.getLogger(__name__)

# Cache the access token for the session
_gcs_token: str | None = None
_gcs_token_time: datetime | None = None


def _get_gcs_token() -> str | None:
    """Get GCS access token via gcloud CLI."""
    global _gcs_token, _gcs_token_time
    # Reuse token if less than 30 min old
    if _gcs_token and _gcs_token_time:
        age = (datetime.now() - _gcs_token_time).total_seconds()
        if age < 1800:
            return _gcs_token
    try:
        result = subprocess.run(
            ["gcloud", "auth", "application-default", "print-access-token"],
            capture_output=True, text=True, timeout=10,
        )
        if result.returncode == 0 and result.stdout.strip():
            _gcs_token = result.stdout.strip()
            _gcs_token_time = datetime.now()
            return _gcs_token
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    return None


def _setup_duckdb_gcs(conn: duckdb.DuckDBPyConnection) -> bool:
    """Configure DuckDB for GCS access. Returns True if successful."""
    try:
        conn.execute("INSTALL httpfs; LOAD httpfs;")
    except duckdb.IOException:
        # Already installed
        conn.execute("LOAD httpfs;")

    conn.execute("SET s3_endpoint='storage.googleapis.com';")
    conn.execute("SET s3_url_style='path';")

    # Try credential chain first
    try:
        conn.execute("CREATE SECRET IF NOT EXISTS gcs_secret (TYPE GCS, PROVIDER CREDENTIAL_CHAIN);")
        # Test if it actually works
        conn.execute(f"SELECT 1")
        return True
    except Exception:
        pass

    # Fall back to gcloud access token with bearer auth
    token = _get_gcs_token()
    if token:
        try:
            conn.execute("DROP SECRET IF EXISTS gcs_secret;")
            conn.execute(f"""
                CREATE SECRET gcs_secret (
                    TYPE GCS,
                    PROVIDER ACCESS_TOKEN,
                    ACCESS_TOKEN '{token}'
                );
            """)
            return True
        except Exception as e:
            logger.warning("Failed to create GCS secret with access token: %s", e)
            # Last resort: set bearer token directly
            conn.execute(f"SET s3_access_key_id='';")
            conn.execute(f"SET s3_secret_access_key='';")
            conn.execute(f"SET http_extra_headers=MAP {{'Authorization': 'Bearer {token}'}};")
            return True

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
