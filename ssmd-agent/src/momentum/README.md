# Momentum Trading Models

Paper-trading momentum models for Kalshi sports markets. Runs three models (volume spike, trade flow imbalance, price acceleration) against live or historical data.

## Live Runner

Connects to NATS JetStream and processes messages in real time:

```bash
cd ssmd-agent

deno run --allow-net --allow-env --allow-read \
  src/cli/main.ts momentum run --config momentum.yaml
```

### Run Options

| Flag | Description | Default |
|------|-------------|---------|
| `--config <path>` | Config file (YAML) | built-in defaults |
| `--stream <name>` | NATS stream | `PROD_KALSHI_SPORTS` |
| `--balance <amount>` | Starting balance ($) | `500` |
| `--nats-url <url>` | NATS URL | `nats://localhost:4222` |

## Backtest

Replays historical JSONL.gz archives from GCS through the same models and prints trading results.

### Prerequisites

- `gcloud` CLI authenticated with access to the archive bucket
- Archives at `gs://<bucket>/<prefix>/<YYYY-MM-DD>/<HHMM>.jsonl.gz`

### Usage

```bash
cd ssmd-agent

# Single date
deno run --allow-net --allow-env --allow-read --allow-run \
  src/cli/main.ts momentum backtest \
  --config momentum.yaml --from 2026-01-24

# Date range
deno run --allow-net --allow-env --allow-read --allow-run \
  src/cli/main.ts momentum backtest \
  --config momentum.yaml --from 2026-01-24 --to 2026-01-26

# Specific dates
deno run --allow-net --allow-env --allow-read --allow-run \
  src/cli/main.ts momentum backtest \
  --config momentum.yaml --dates 2026-01-20,2026-01-24,2026-01-26
```

### Backtest Options

| Flag | Description | Default |
|------|-------------|---------|
| `--config <path>` | Config file (YAML) — same as live runner | **required** |
| `--from <YYYY-MM-DD>` | Start date | |
| `--to <YYYY-MM-DD>` | End date | same as `--from` |
| `--dates <d1,d2,...>` | Specific dates (alternative to `--from`/`--to`) | |
| `--bucket <name>` | GCS bucket | `ssmd-archive` |
| `--prefix <path>` | GCS prefix | `kalshi/sports` |

### Output

The backtest prints the same logs as the live runner:
- `[ACTIVATED]` — ticker reached the dollar-volume activation threshold
- `[ENTRY]` — model opened a position
- `[EXIT]` — position closed (take-profit, stop-loss, time-stop, or force-close)
- Summary table with per-model P&L at the end

## Config

See `momentum.yaml` in the ssmd-agent root for the full config with all model parameters, portfolio settings, and fee schedules.
