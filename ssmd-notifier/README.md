# ssmd-notifier

Routes NATS signal fires to notification destinations.

## Quick Start

```bash
# Set environment
export NATS_URL=nats://localhost:4222
export STREAM=SIGNALS
export CONSUMER=notifier
export DESTINATIONS_CONFIG=./destinations.json

# Run
deno task start
```

## Configuration

**Environment Variables:**

| Var | Required | Description |
|-----|----------|-------------|
| `NATS_URL` | Yes | NATS server URL |
| `STREAM` | Yes | JetStream stream name (e.g., SIGNALS) |
| `CONSUMER` | Yes | Durable consumer name |
| `FILTER_SUBJECT` | No | Subject filter (e.g., `signals.volume-1m-30min.>`) |
| `DESTINATIONS_CONFIG` | Yes | Path to destinations.json |

**destinations.json:**

```json
[
  {
    "name": "all-alerts",
    "type": "ntfy",
    "config": {
      "server": "https://ntfy.sh",
      "topic": "my-alerts",
      "priority": "default"
    }
  },
  {
    "name": "volume-only",
    "type": "ntfy",
    "config": { "topic": "volume-alerts", "priority": "high" },
    "match": { "field": "signalId", "operator": "contains", "value": "volume" }
  }
]
```

## Match Rules

Match rules filter which signal fires go to which destinations.

| Field | Description |
|-------|-------------|
| `signalId` | Signal identifier (e.g., `volume-1m-30min`) |
| `ticker` | Market ticker |

| Operator | Description |
|----------|-------------|
| `eq` | Exact match |
| `contains` | Substring match |

No match rule = route all fires to destination.

## Endpoints

- `GET /health` - Liveness probe
- `GET /ready` - Readiness probe
- `GET /metrics` - Prometheus metrics

## Development

```bash
deno task test    # Run tests
deno task check   # Type check
```

## Deployment

Managed by Notifier CRD. See ssmd-operators.

Tag format: `notifier-v*` (e.g., `notifier-v0.1.0`)
