# Runtime Framework Design

## Overview

Add a runtime framework to ssmd that reads the GitOps metadata (feeds, schemas, environments) and actually collects market data. Proves the metadata model works end-to-end.

## Command

```
ssmd run <environment> --config-dir <path>
```

- `--config-dir` is required (no default)
- Reads environment config, loads referenced feed, resolves keys from env vars
- Connects to data source, writes raw messages to local storage
- Exposes health/metrics HTTP endpoints
- Exits on disconnect (K8s handles restart)

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  ssmd run kalshi-dev            │
├─────────────────────────────────────────────────┤
│  Config Loader                                  │
│  - Reads environments/<env>.yaml                │
│  - Loads referenced feed                        │
│  - Resolves keys from env vars                  │
├─────────────────────────────────────────────────┤
│  Connector (interface)                          │
│  - WebSocketConnector for Kalshi                │
│  - Connects, authenticates, receives messages   │
│  - On disconnect: exit process                  │
├─────────────────────────────────────────────────┤
│  Writer (interface)                             │
│  - FileWriter for JSONL                         │
│  - Date-partitioned files                       │
├─────────────────────────────────────────────────┤
│  HTTP Server (:8080)                            │
│  - GET /health - liveness probe                 │
│  - GET /ready  - readiness probe                │
│  - GET /metrics - Prometheus format             │
└─────────────────────────────────────────────────┘
```

## Framework Interfaces

```go
// Connector interface - WebSocketConnector, RESTPoller, etc.
type Connector interface {
    Connect(ctx context.Context) error
    Messages() <-chan []byte
    Close() error
}

// Writer interface - FileWriter, S3Writer, KafkaWriter, etc.
type Writer interface {
    Write(ctx context.Context, msg []byte) error
    Close() error
}

// KeyResolver interface - EnvResolver, VaultResolver, etc.
type KeyResolver interface {
    Resolve(source string) (map[string]string, error)
}
```

## Package Structure

```
internal/
  runtime/
    interfaces.go      # Connector, Writer, KeyResolver interfaces
    runner.go          # Main run loop, wires components together
  connector/
    websocket.go       # WebSocket implementation of Connector
  writer/
    file.go            # JSONL file implementation of Writer
  resolver/
    env.go             # Environment variable KeyResolver
  server/
    health.go          # HTTP health/metrics endpoints
```

## Data Flow

```
Kalshi WebSocket
       │
       ▼
┌─────────────────┐
│ Connector       │ Receives raw JSON messages
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Runner          │ Adds metadata (timestamp, source)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Writer          │ Appends to JSONL file
└─────────────────┘
```

## Storage Format

**File Structure**

```
{storage.path}/
  2025-12-22/
    kalshi.jsonl
  2025-12-23/
    kalshi.jsonl
```

**JSONL Format** (one message per line)

```json
{"ts":"2025-12-22T10:30:00Z","feed":"kalshi","data":{...raw message...}}
```

## Health & Metrics

**Endpoints**

```
GET /health   → {"status":"ok"} or {"status":"error","reason":"..."}
GET /ready    → {"status":"ready"} or {"status":"not_ready"}
GET /metrics  → Prometheus format
```

**Health Logic**
- `/health` - Returns OK if process is running (liveness)
- `/ready` - Returns OK if WebSocket is connected (readiness)

**Metrics**

```
ssmd_messages_total{feed="kalshi"} 12345
ssmd_errors_total{feed="kalshi",type="write"} 0
ssmd_connected{feed="kalshi"} 1
ssmd_last_message_timestamp{feed="kalshi"} 1703245800
```

## K8s Deployment

**GitOps Flow**

- `exchanges/` directory in git → ConfigMap mounted at `/etc/ssmd/exchanges`
- Secrets (API keys) → K8s Secret → injected as env vars
- `ssmd run <env> --config-dir /etc/ssmd/exchanges`

**Manifest Example**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: kalshi-collector
spec:
  replicas: 1
  template:
    spec:
      containers:
      - name: collector
        image: ssmd:latest
        args: ["run", "kalshi-dev", "--config-dir", "/etc/ssmd/exchanges"]
        ports:
        - containerPort: 8080
        envFrom:
        - secretRef:
            name: kalshi-keys
        volumeMounts:
        - name: config
          mountPath: /etc/ssmd/exchanges
        livenessProbe:
          httpGet: {path: /health, port: 8080}
        readinessProbe:
          httpGet: {path: /ready, port: 8080}
      volumes:
      - name: config
        configMap:
          name: ssmd-config
```

## Phase 1 Scope

**Building:**
- `ssmd run <env> --config-dir <path>` command
- Framework interfaces: Connector, Writer, KeyResolver
- WebSocketConnector (for Kalshi)
- FileWriter (JSONL, date-partitioned)
- EnvResolver (reads from env vars)
- HTTP server with /health, /ready, /metrics
- Fail-fast on disconnect

**Not building yet:**
- REST polling connector
- S3/Parquet writer
- Vault key resolver
- Schema validation
- NATS transport
- Scheduling/rate limiting
