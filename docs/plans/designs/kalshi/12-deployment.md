# ssmd: Kalshi Design - Deployment & Observability

## CLI Commands

```bash
# Environment management
ssmd env create kalshi-prod --feed kalshi --schema trade:v1
ssmd env validate exchanges/environments/kalshi.yaml
ssmd env apply exchanges/environments/kalshi.yaml

# Market operations
ssmd secmaster list kalshi-prod
ssmd secmaster sync kalshi-prod
ssmd secmaster show kalshi-prod INXD-25-B4000

# Operations
ssmd day start kalshi-prod       # Trigger startup workflow
ssmd day end kalshi-prod         # Trigger teardown workflow
ssmd day status kalshi-prod      # Current system state

# Data operations
ssmd data replay --date 2025-12-14 --symbol INXD-25-B4000
ssmd data export --date 2025-12-14 --format parquet
```

## ssmd-data API

HTTP API for accessing archived market data. Used by ssmd-agent and other tooling.

### Running Locally

```bash
# Build and run
make data-build
SSMD_DATA_PATH=./testdata SSMD_API_KEY=dev ./bin/ssmd-data

# Or via Makefile (uses default dev settings)
make data-run
```

### Docker Deployment

```bash
# Build image
docker build -t ssmd-data -f cmd/ssmd-data/Dockerfile .

# Run container
docker run -d \
  -p 8080:8080 \
  -v /path/to/archive:/data:ro \
  -e SSMD_DATA_PATH=/data \
  -e SSMD_API_KEY=your-api-key \
  ssmd-data
```

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| SSMD_DATA_PATH | Yes | - | Path to archived data (local path or `gs://bucket/prefix`) |
| SSMD_API_KEY | Yes | - | API key for authentication |
| SSMD_DATA_PORT | No | 8080 | HTTP server port |

### Endpoints

| Endpoint | Description |
|----------|-------------|
| GET /health | Health check |
| GET /datasets | List available feeds and dates |
| GET /datasets/{feed}/{date}/sample | Sample data with optional filters |
| GET /schema/{feed}/{type} | Get message schema |
| GET /builders | List available state builders |

### Authentication

All endpoints except `/health` require the `X-API-Key` header:

```bash
curl -H "X-API-Key: your-api-key" http://localhost:8080/datasets
```

## ssmd-agent REPL

LangGraph-based AI agent for exploring data and creating trading signals.

### Prerequisites

```bash
# Install Deno
curl -fsSL https://deno.land/install.sh | sh
export PATH="$HOME/.deno/bin:$PATH"
```

### Running

```bash
# Run agent REPL
cd ssmd-agent
deno task agent

# Or via Makefile
make agent-run
```

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| ANTHROPIC_API_KEY | Yes | - | Claude API key |
| SSMD_DATA_URL | No | http://localhost:8080 | ssmd-data API URL |
| SSMD_API_KEY | No | dev | API key for ssmd-data |
| SSMD_MODEL | No | claude-sonnet-4-20250514 | Claude model to use |

### Skills

Skills are loaded from `ssmd-agent/skills/*.md`:

| Skill | Description |
|-------|-------------|
| explore-data | Guide for exploring archived datasets |
| monitor-spread | Template for spread monitoring signals |
| interpret-backtest | Guide for analyzing backtest results |
| custom-signal | Template for custom signal logic |

### Tools

The agent has access to these tools:

| Tool | Description |
|------|-------------|
| list_datasets | Query available feeds and dates |
| sample_data | Fetch sample messages from archives |
| get_schema | Get schema for a message type |
| list_builders | List available state builders |
| orderbook_builder | Process messages through OrderBookBuilder |
| run_backtest | Run backtest with signal code |
| deploy_signal | Deploy signal to signals/ directory |

## Environment Definition

```yaml
# exchanges/environments/kalshi-prod.yaml
name: kalshi-prod
feed: kalshi
schema: trade:v1

schedule:
  timezone: UTC
  day_start: "00:10"
  day_end: "00:00"
  auto_roll: true

keys:
  kalshi:
    type: api_key
    required: true
    fields: [api_key, api_secret]
    source: sealed-secret/kalshi-creds

transport:
  type: nats
  url: nats://nats.ssmd.local:4222
  jetstream:
    enabled: true
    stream: ssmd-kalshi

storage:
  type: s3
  endpoint: http://garage.ssmd.local:3900
  buckets:
    raw: ssmd-raw
    normalized: ssmd-normalized

cache:
  type: redis
  url: redis://redis.ssmd.local:6379
```

## Observability

### Metrics (Prometheus)

```
# Connector
ssmd_connector_messages_received_total{feed="kalshi",type="trade"}
ssmd_connector_messages_published_total{feed="kalshi"}
ssmd_connector_lag_seconds{feed="kalshi"}
ssmd_connector_errors_total{feed="kalshi",error_type="parse"}

# Gateway
ssmd_gateway_clients_connected
ssmd_gateway_messages_sent_total{type="trade"}
ssmd_gateway_subscriptions_active{symbol="INXD-25-B4000"}

# Archiver
ssmd_archiver_bytes_written_total{bucket="raw"}
ssmd_archiver_files_written_total{bucket="normalized"}
```

### Alerts

```yaml
# Critical: No data flowing
- alert: ConnectorNoData
  expr: rate(ssmd_connector_messages_received_total[5m]) == 0
  for: 2m
  labels:
    severity: critical

# Warning: High lag
- alert: ConnectorHighLag
  expr: ssmd_connector_lag_seconds > 5
  for: 1m
  labels:
    severity: warning
```

### Logs

Structured JSON to stdout, collected with Loki:

```json
{"level":"info","ts":"2025-12-14T00:10:00Z","component":"connector","msg":"connected to kalshi","symbols":42}
{"level":"info","ts":"2025-12-14T00:10:01Z","component":"connector","msg":"trade","ticker":"INXD-25-B4000","price":0.45}
```

## Dependencies

### Rust Crates

| Dependency | Version | Purpose |
|------------|---------|---------|
| tokio | 1.x | Async runtime |
| tungstenite | 0.21 | WebSocket client |
| capnp | 0.18 | Cap'n Proto |
| async-nats | 0.33 | NATS client |
| serde | 1.x | JSON serialization |
| tracing | 0.1 | Structured logging |
| aws-sdk-s3 | 1.x | S3 storage |
| redis | 0.24 | Redis client |

### Go Modules

| Dependency | Purpose |
|------------|---------|
| github.com/spf13/cobra | CLI framework |
| github.com/temporalio/sdk-go | Temporal workflows |
| github.com/nats-io/nats.go | NATS client |
| gopkg.in/yaml.v3 | YAML parsing |

### Deno Dependencies (ssmd-agent)

| Dependency | Version | Purpose |
|------------|---------|---------|
| @langchain/langgraph | 0.2.57 | Agent workflow framework |
| @langchain/anthropic | 0.3.18 | Claude integration |
| @langchain/core | 0.3.31 | LangChain core types |
| zod | 3.24.1 | Schema validation |

## Infrastructure Requirements

### Existing (ready to use)

| Service | Notes |
|---------|-------|
| NATS + JetStream | File persistence, streams configured |
| Redis | For cache and secmaster |
| Sealed Secrets | For ssmd secrets |
| Traefik | Ingress with TLS |
| Prometheus/Grafana/Loki | Monitoring stack |

### Needs Deployment

| Service | Purpose |
|---------|---------|
| ArgoCD | GitOps deployment for ssmd |
| Temporal | Workflow orchestration for daily startup/teardown |

### Storage Strategy (Pre-Garage)

Until S3-compatible storage (Garage) is deployed:

```yaml
# Initial: Local storage
storage:
  type: local
  path: /var/lib/ssmd/storage

# Future: Garage S3
storage:
  type: s3
  endpoint: http://garage.brooklyn.local:3900
  buckets:
    raw: ssmd-raw
    normalized: ssmd-normalized
```

The Storage trait abstraction allows seamless migration when ready.

## Infrastructure Setup

```bash
# 1. Deploy ArgoCD
kubectl create namespace argocd
kubectl apply -n argocd -f https://raw.githubusercontent.com/argoproj/argo-cd/stable/manifests/install.yaml

# 2. Configure ArgoCD for ssmd repo
argocd app create ssmd \
  --repo https://github.com/your-org/ssmd.git \
  --path k8s/overlays/prod \
  --dest-server https://kubernetes.default.svc \
  --dest-namespace ssmd

# 3. Deploy Temporal
helm repo add temporal https://temporal.io/helm-charts
helm install temporal temporal/temporal \
  --namespace temporal \
  --create-namespace \
  --set server.replicaCount=1 \
  --set cassandra.enabled=false \
  --set postgresql.enabled=true

# 4. Create ssmd namespace and sealed secrets
kubectl create namespace ssmd
kubeseal --fetch-cert > ssmd-sealed-secrets-cert.pem
```
