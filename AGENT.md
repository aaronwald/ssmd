# ssmd-agent

Local development tool for creating market data signals using LangGraph + Claude.

**This runs on your laptop**, not in Kubernetes.

## Setup

```bash
# Install Deno 2.x
curl -fsSL https://deno.land/install.sh | sh

# Set environment
export SSMD_API_URL=https://ssmd-data.your-domain.com
export SSMD_DATA_API_KEY=<api-key>
export SSMD_MODEL="anthropic/claude-sonnet-4"  # optional
```

Get your API key from the ssmd-data deployment:

```bash
kubectl get secret -n ssmd ssmd-data-credentials \
  -o jsonpath='{.data.api-key}' | base64 -d
```

## Running

```bash
cd ssmd-agent
deno task agent
```

Non-interactive mode:

```bash
deno task agent --prompt "List markets in Economics category"
```

## Tools

### Data Discovery

| Tool | Description |
|------|-------------|
| `list_datasets` | Available feeds and dates |
| `list_tickers` | Tickers in a dataset |
| `sample_data` | Fetch raw market records |
| `get_schema` | Message type schema |

### Secmaster

| Tool | Description |
|------|-------------|
| `list_markets` | Markets with filters |
| `get_market` | Market details |
| `list_events` | Events with market counts |
| `list_series` | Series groups |
| `get_fees` | Fee schedule |

### State Builders

| Tool | Description |
|------|-------------|
| `orderbook_builder` | Records → orderbook snapshots |
| `price_history_builder` | Trades → VWAP, returns, volatility |
| `volume_profile_builder` | Volume over sliding window |

### Backtest & Deploy

| Tool | Description |
|------|-------------|
| `run_backtest` | Evaluate signal against states |
| `deploy_signal` | Write signal file and git commit |

## Example Session

```
ssmd-agent> What datasets are available?
[tool] list_datasets()
  → 3 datasets: kalshi/2025-12-23, kalshi/2025-12-24, kalshi/2025-12-25

ssmd-agent> Create a spread alert for >5%
[tool] sample_data(...)
[tool] orderbook_builder(...)
[tool] run_backtest(...)
  → 3 fires, 0 errors

ssmd-agent> Deploy it
[tool] deploy_signal(code="...", path="spread-alert-5pct.ts")
  → Committed: a1b2c3d
```

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `SSMD_API_URL` | No | `http://localhost:8080` | ssmd-data-ts URL |
| `SSMD_DATA_API_KEY` | Yes | - | API key |
| `SSMD_MODEL` | No | `anthropic/claude-sonnet-4` | Model (OpenRouter format) |
| `SSMD_SKILLS_PATH` | No | `./skills` | Skills directory |
| `SSMD_SIGNALS_PATH` | No | `./signals` | Signal output directory |

## Troubleshooting

**401 Unauthorized**: Check API key matches ssmd-data-credentials secret.

**Connection refused**: Ensure ssmd-data-ts is accessible. Port forward if needed:

```bash
kubectl port-forward -n ssmd svc/ssmd-data-ts 8080:8080
```

**No datasets**: Archiver hasn't produced data yet. Check archiver logs.

## Local Testing

For testing without a cluster:

```bash
# Create test data
mkdir -p /tmp/ssmd-data/kalshi/2025-12-25
echo '{"type":"ticker","ticker":"TEST-001","yes_bid":0.45,"yes_ask":0.55}' \
  | gzip > /tmp/ssmd-data/kalshi/2025-12-25/test.jsonl.gz

# Run ssmd-data-ts locally
SSMD_DATA_PATH=/tmp/ssmd-data SSMD_API_KEY=test deno task data

# Run agent
SSMD_API_URL=http://localhost:8080 SSMD_DATA_API_KEY=test deno task agent
```
