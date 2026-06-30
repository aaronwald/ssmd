# notebooks

Local Jupyter Lab notebooks for exploring ssmd market data with **DuckDB**.

The ssmd public data API (`https://api.varshtat.com`) exposes two things these
notebooks use:

- **Live JSON** — rolling 1-minute OHLCV bars and the symbols that have them
  (`/v1/data/ohlcv/1m`, `/v1/data/ohlcv/1m/symbols`).
- **Archived Parquet** — daily OHLCV files on GCS, fetched as short-lived
  **signed URLs** (`/v1/data/download`). DuckDB reads these URLs directly over
  `httpfs`, which is the main pattern here.

## Setup

Uses [uv](https://docs.astral.sh/uv/) (`brew install uv` if you don't have it).

```bash
cd notebooks
uv venv                          # create .venv
uv pip install -r requirements.txt

cp .env.example .env             # then paste your SSMD_API_KEY into .env
```

Your key needs the `datasets:read` scope and access to the feeds you query.
Manage keys at <https://harman.varshtat.com> → **Admin → API Keys**.

## Run

`uv run` launches Jupyter inside the venv; `--env-file .env` exports your key
into the kernel's environment (no `python-dotenv` needed):

```bash
uv run --env-file .env jupyter lab
```

Open **`ssmd_duckdb_quickstart.ipynb`** and run the cells top to bottom.

> Running a single notebook headless (no UI):
> `uv run --env-file .env jupyter execute ssmd_duckdb_quickstart.ipynb`

## What's in the quickstart

1. Config — reads `SSMD_API_KEY` / `SSMD_API_BASE` from the environment
   (exported by `uv run --env-file .env`).
2. Discover live `kraken-spot` symbols that have 1m bars.
3. Pull live 1m bars into a DuckDB relation and compute VWAP / returns.
4. **DuckDB over `httpfs`** — read a `hols` daily OHLCV Parquet straight from a
   signed GCS URL, no download step.
5. An aggregation example (top symbols by volume) on that Parquet.
6. Tips — persisting a local `.duckdb`, reading many days at once, the
   download-then-query pattern for large files.

## Feeds & data availability (as of 2026-06)

| Feed | Live `ohlcv/1m` JSON | Downloadable Parquet (`/v1/data/download`) |
|------|----------------------|--------------------------------------------|
| `kraken-spot` | ✅ pairs like `BTC/USDT` | ❌ (raw feed; not exposed here) |
| `massive` (US equities) | ✅ `AAPL`, `SPY` … | ✅ raw feed Parquet (needs `massive` feed access) |
| `hols` (crypto daily OHLCV) | — | ✅ `ohlcv-1m-binance`, `ohlcv-5m-binance`, `ohlcv-5m-kraken`, `tickers-reference.csv` |

Notes:
- `kraken-spot` symbols are **pairs with a slash** (`BTC/USDT`). URL-encode the
  slash as `%2F` when building a URL by hand; the `requests` calls here encode
  it for you.
- `hols` Parquet timestamps (`date`, `date_close`) are UTC minute/bar
  boundaries; `unix` / `close_unix` are epoch seconds.
- Signed URLs expire (default 12h). Re-run the download cell to refresh them.
