# ssmd-agent: Signal Development Assistant

`ssmd-agent` is a local development tool for creating market data signals. It uses LangGraph with Claude to help you explore data, generate signal code, backtest, and deploy.

**This is NOT a deployed service** - it runs on your laptop and connects to the ssmd-data API on your homelab.

## Architecture

```
YOUR LAPTOP                              HOMELAB (Kubernetes)
┌─────────────────────────────────┐      ┌─────────────────────────────────┐
│  ssmd-agent (Deno REPL)         │      │  ssmd-connector → NATS          │
│     │                           │      │         ↓                       │
│     │ HTTP (X-API-Key)          │      │  ssmd-archiver → JSONL.gz       │
│     └───────────────────────────│──────│─→ ssmd-data API (:8080)         │
│                                 │      │                                 │
│  You: "Create a spread alert"  │      └─────────────────────────────────┘
│  Agent: Uses tools, generates   │
│         code, runs backtest     │
└─────────────────────────────────┘
```

## Prerequisites

1. **Deno 2.x** installed locally
   ```bash
   curl -fsSL https://deno.land/install.sh | sh
   ```

2. **ssmd-data API** running on homelab (see [DEPLOYMENT.md](DEPLOYMENT.md))
   - The agent uses ssmd-data as an LLM proxy (routes to OpenRouter)
   - No direct Anthropic API key needed - authentication handled by ssmd-data

3. **Archived data** - the archiver must have produced JSONL.gz files (for data exploration tools)

## Setup

### 1. Get ssmd-data API credentials

```bash
# From your homelab
kubectl get secret -n ssmd ssmd-data-credentials \
  -o jsonpath='{.data.api-key}' | base64 -d
```

### 2. Expose ssmd-data API

```bash
# Option A: Port forward (temporary)
kubectl port-forward -n ssmd svc/ssmd-data 8080:8080

# Option B: Use Ingress URL if configured
# e.g., https://ssmd-data.homelab.local
```

### 3. Set environment variables

```bash
export SSMD_API_URL=https://ssmd-data.varshtat.com  # or your ingress URL
export SSMD_DATA_API_KEY=<api-key-from-step-1>
export SSMD_MODEL="anthropic/claude-sonnet-4.5"     # optional, this is the default
```

## Running the Agent

```bash
cd ssmd-agent
deno task agent
```

### Non-Interactive Mode

For testing or scripting, use the `--prompt` flag:

```bash
deno task agent --prompt "What tools are available?"
deno task agent --prompt "List markets in the Economics category"
```

You'll see:
```
ssmd-agent v0.1.0
Type 'quit' to exit

ssmd-agent>
```

## Example Session

```
ssmd-agent> What datasets are available?

[tool] list_datasets()
  → 3 items

I found 3 datasets:
- kalshi / 2025-12-23 - 45,230 records, 127 tickers
- kalshi / 2025-12-24 - 52,100 records, 134 tickers
- kalshi / 2025-12-25 - 38,900 records, 121 tickers

ssmd-agent> Sample some orderbook data from Dec 25

[tool] sample_data(feed="kalshi", date="2025-12-25", type="orderbook", limit=10)
  → 10 items

Here are 10 orderbook records from Kalshi on 2025-12-25:
...

ssmd-agent> Create a signal that fires when spread exceeds 5%

[tool] orderbook_builder(records=[...])
  → 10 snapshots

[tool] run_backtest(signalCode="...", states=[...])
  → 3 fires, 0 errors

I've created a spread alert signal. The backtest shows:
- 3 fires on 10 records (would need more data for realistic test)
- No errors

Here's the signal code:
```typescript
export const signal = {
  id: "spread-alert-5pct",
  ...
};
```

Would you like me to deploy this signal?

ssmd-agent> Yes, deploy it

[tool] deploy_signal(code="...", path="spread-alert-5pct.ts")
  → Committed: a1b2c3d

Deployed to signals/spread-alert-5pct.ts (commit a1b2c3d)
```

## Available Tools

### Calendar Tools
| Tool | Description |
|------|-------------|
| `get_today` | Get today's date in YYYY-MM-DD format (UTC) |

### Data Discovery Tools
| Tool | Description |
|------|-------------|
| `list_datasets` | List available feeds and dates |
| `list_tickers` | List tickers in a dataset |
| `sample_data` | Fetch raw market records |
| `get_schema` | Get field definitions for message types |
| `list_builders` | List available state builders |

### Secmaster Tools
| Tool | Description |
|------|-------------|
| `list_markets` | List markets with filters (category, status, series, point-in-time) |
| `get_market` | Get details for a specific market |
| `list_events` | List events with market counts |
| `get_event` | Get event details including markets |
| `list_series` | List series (groups of related markets) |
| `get_series` | Get series details |
| `get_fees` | Get fee schedule for a tier |
| `get_fee_schedule` | Get fee schedule for a series ticker |

### State Builder Tools
| Tool | Description |
|------|-------------|
| `orderbook_builder` | Process records into orderbook state snapshots |
| `price_history_builder` | Process trades into price history with VWAP, returns, volatility |
| `volume_profile_builder` | Track contract and USD volume over sliding time window |

### Backtest & Deploy Tools
| Tool | Description |
|------|-------------|
| `run_backtest` | Evaluate signal code against states |
| `deploy_signal` | Write signal file and git commit |

## Skills

The agent loads skills from `ssmd-agent/skills/*.md`:

| Skill | Purpose |
|-------|---------|
| `explore-data` | How to discover and understand data |
| `monitor-spread` | Generate spread monitoring signals |
| `interpret-backtest` | Analyze backtest results |
| `custom-signal` | Template for custom signal logic |

## Testing Without Homelab

For local testing without a deployed homelab:

### 1. Create test data

```bash
mkdir -p /tmp/ssmd-data/kalshi/2025-12-25

# Create a minimal manifest
cat > /tmp/ssmd-data/kalshi/2025-12-25/manifest.json << 'EOF'
{
  "feed": "kalshi",
  "date": "2025-12-25",
  "tickers": ["INXD-25001"],
  "message_types": ["ticker", "orderbook"],
  "files": [{"name": "test.jsonl.gz", "records": 3, "bytes": 500}],
  "has_gaps": false
}
EOF

# Create test records (gzipped JSONL)
echo '{"type":"ticker","ticker":"INXD-25001","yes_bid":0.45,"yes_ask":0.55,"ts":1735084800000}
{"type":"ticker","ticker":"INXD-25001","yes_bid":0.46,"yes_ask":0.54,"ts":1735084801000}
{"type":"ticker","ticker":"INXD-25001","yes_bid":0.40,"yes_ask":0.60,"ts":1735084802000}' \
  | gzip > /tmp/ssmd-data/kalshi/2025-12-25/test.jsonl.gz
```

### 2. Run ssmd-data locally

```bash
export SSMD_DATA_PATH=/tmp/ssmd-data
export SSMD_API_KEY=test-key
export PORT=8080

go run ./cmd/ssmd-data
```

### 3. Run agent against local API

```bash
export SSMD_DATA_URL=http://localhost:8080
export SSMD_DATA_API_KEY=test-key
export ANTHROPIC_API_KEY=sk-ant-...

cd ssmd-agent
deno task agent
```

## Verifying ssmd-data Connection

Before running the agent, verify the API is accessible:

```bash
# Health check (no auth required)
curl $SSMD_DATA_URL/health
# → {"status":"ok"}

# List datasets (requires auth)
curl -H "X-API-Key: $SSMD_DATA_API_KEY" $SSMD_DATA_URL/datasets
# → [{"feed":"kalshi","date":"2025-12-25",...}]

# Sample data
curl -H "X-API-Key: $SSMD_DATA_API_KEY" \
  "$SSMD_DATA_URL/datasets/kalshi/2025-12-25/sample?limit=5"
# → [{...}, {...}, ...]
```

## Testing the Agent

### Quick Verification

Test that the agent starts and can use tools:

```bash
cd ssmd-agent
export SSMD_DATA_API_KEY=<your-api-key>
export SSMD_API_URL=https://ssmd-data.varshtat.com
export SSMD_MODEL="anthropic/claude-sonnet-4.5"

# Test tool listing
deno task agent --prompt "What tools are available?"

# Test secmaster access
deno task agent --prompt "How many active markets are in the Economics category?"

# Test date tool
deno task agent --prompt "What is today's date?"
```

### Expected Output

A successful test shows:
- Agent version banner
- Audit log file path
- Tool invocation results (if tools are called)
- Token usage: `[tokens] in: X, out: Y`

---

## Troubleshooting

### "SSMD_DATA_API_KEY required"

Set the environment variable:
```bash
export SSMD_DATA_API_KEY=your-key
```

### "API error: 401"

API key mismatch. Verify:
```bash
curl -H "X-API-Key: $SSMD_DATA_API_KEY" $SSMD_DATA_URL/datasets
```

### "API error: 404 dataset not found"

No archived data for that feed/date. Check:
```bash
curl -H "X-API-Key: $SSMD_DATA_API_KEY" $SSMD_DATA_URL/datasets
```

### Connection refused

ssmd-data not running or not accessible. Check:
- Port forward is active: `kubectl port-forward -n ssmd svc/ssmd-data 8080:8080`
- Or ssmd-data is running locally: `go run ./cmd/ssmd-data`

## Configuration

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `SSMD_API_URL` | No | `http://localhost:8080` | ssmd-data API URL |
| `SSMD_DATA_API_KEY` | Yes | - | API key for ssmd-data (also used for LLM proxy) |
| `SSMD_MODEL` | No | `anthropic/claude-sonnet-4` | Model in OpenRouter format |
| `SSMD_SKILLS_PATH` | No | `./skills` | Path to skills directory |
| `SSMD_PROMPTS_PATH` | No | `./prompts` | Path to prompt templates |
| `SSMD_SIGNALS_PATH` | No | `./signals` | Path to deploy signals |

## What's NOT Implemented Yet

| Feature | Status |
|---------|--------|
| Memory persistence | Agent has no memory between sessions (SQLite checkpointer planned) |
| Streaming responses | LLM responses are not streamed (proxy limitation) |
