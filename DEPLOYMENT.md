# SSMD Deployment Guide

Deploy ssmd components to Kubernetes.

## Components

| Component | Image | Purpose |
|-----------|-------|---------|
| **ssmd-connector** | `ghcr.io/<owner>/ssmd-connector` | Kalshi WebSocket → NATS (raw JSON) |
| **ssmd-archiver** | `ghcr.io/<owner>/ssmd-archiver` | NATS → JSONL.gz files |
| **ssmd-data** | `ghcr.io/<owner>/ssmd-data` | HTTP API for archived data + secmaster |

> **Note:** `ssmd-agent` is a local development tool, not a deployed service. See [AGENT.md](AGENT.md) for usage.

### Optional Dependencies

| Component | Purpose | Required For |
|-----------|---------|--------------|
| **PostgreSQL** | Market metadata storage | Secmaster endpoints (`/markets`, `/fees`) |

---

## ssmd-connector

Deploy ssmd-connector to Kubernetes. Publishes raw JSON to NATS JetStream.

| Mode | Transport | Output | Use Case |
|------|-----------|--------|----------|
| **NATS** | `nats` | Raw JSON to JetStream | Real-time streaming |

**Subjects:**
- `{env}.{feed}.json.trade.{ticker}` - Trade executions
- `{env}.{feed}.json.ticker.{ticker}` - Price updates
- `{env}.{feed}.json.orderbook.{ticker}` - Orderbook updates

## Prerequisites

- Kubernetes cluster with NATS JetStream (for NATS mode)
- Container registry access (GHCR or other)
- `kubectl` configured for target cluster
- `kubeseal` CLI (if using Sealed Secrets)

## Container Image

### Build Locally

```bash
cd ssmd-rust
docker build -t ssmd-connector:latest .
```

### GitHub Actions

Tags trigger automatic builds:

```bash
git tag v0.3.0
git push origin v0.3.0
# Image pushed to ghcr.io/<owner>/ssmd-connector:0.3.0
```

## Kubernetes Resources

### Namespace

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: ssmd
  labels:
    name: ssmd
```

### Secret (Kalshi Credentials)

```bash
kubectl create secret generic ssmd-kalshi-credentials \
  --namespace=ssmd \
  --from-literal=api-key="$KALSHI_API_KEY" \
  --from-file=private-key=/path/to/kalshi-private-key.pem
```

---

## Deployment: NATS Streaming

Publishes raw JSON market data to NATS JetStream.

### ConfigMap

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: ssmd-nats-config
  namespace: ssmd
data:
  feeds/kalshi.yaml: |
    name: kalshi
    feed_type: websocket
    versions:
      - version: "1.0"
        endpoint: wss://trading-api.kalshi.com/trade-api/ws/v2
        protocol:
          transport: websocket
          message: json

  environments/prod-nats.yaml: |
    name: prod
    feed: kalshi
    schema: "trade:v1"
    transport:
      transport_type: nats
      url: nats://nats.nats.svc.cluster.local:4222
    storage:
      storage_type: local
```

### Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ssmd-connector-nats
  namespace: ssmd
spec:
  replicas: 1
  selector:
    matchLabels:
      app: ssmd-connector-nats
  template:
    metadata:
      labels:
        app: ssmd-connector-nats
    spec:
      containers:
        - name: connector
          image: ghcr.io/<owner>/ssmd-connector:0.3.0
          args:
            - "--feed"
            - "/config/feeds/kalshi.yaml"
            - "--env"
            - "/config/environments/prod-nats.yaml"
          ports:
            - containerPort: 8080
              name: health
          env:
            - name: KALSHI_API_KEY
              valueFrom:
                secretKeyRef:
                  name: ssmd-kalshi-credentials
                  key: api-key
            - name: KALSHI_PRIVATE_KEY
              valueFrom:
                secretKeyRef:
                  name: ssmd-kalshi-credentials
                  key: private-key
            - name: RUST_LOG
              value: "info"
          volumeMounts:
            - name: config
              mountPath: /config
              readOnly: true
          resources:
            requests:
              cpu: 100m
              memory: 128Mi
            limits:
              cpu: 500m
              memory: 512Mi
          livenessProbe:
            httpGet:
              path: /health
              port: health
            initialDelaySeconds: 30
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /health
              port: health
            initialDelaySeconds: 10
            periodSeconds: 5
      volumes:
        - name: config
          configMap:
            name: ssmd-nats-config
```

### NATS JetStream Stream

```bash
nats stream add PROD_KALSHI \
  --subjects "prod.kalshi.>" \
  --retention limits \
  --max-age 1h \
  --storage file \
  --replicas 1 \
  --discard old \
  -s nats://<nats-host>:4222
```

---

## ssmd-archiver

Subscribes to NATS JetStream and writes JSONL.gz files to disk with configurable rotation.

**Output:** `/data/ssmd/{feed}/{date}/{HHMM}.jsonl.gz` + `manifest.json`

### Container Image

```bash
# Build locally
cd ssmd-rust
docker build -f crates/ssmd-archiver/Dockerfile -t ssmd-archiver:latest .

# Or use GHCR (tags trigger builds)
git tag v0.3.0
git push origin v0.3.0
# Image pushed to ghcr.io/<owner>/ssmd-archiver:0.3.0
```

### ConfigMap

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: ssmd-archiver-config
  namespace: ssmd
data:
  archiver.yaml: |
    nats:
      url: nats://nats.nats.svc.cluster.local:4222
      stream: PROD_KALSHI
      consumer: archiver-kalshi
      filter: "prod.kalshi.json.>"

    storage:
      path: /data/ssmd

    rotation:
      interval: 15m   # 15m for testing, 1h or 1d for production
```

### Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ssmd-archiver
  namespace: ssmd
spec:
  replicas: 1
  selector:
    matchLabels:
      app: ssmd-archiver
  template:
    metadata:
      labels:
        app: ssmd-archiver
    spec:
      containers:
        - name: archiver
          image: ghcr.io/<owner>/ssmd-archiver:0.3.0
          args:
            - "--config"
            - "/config/archiver.yaml"
          env:
            - name: RUST_LOG
              value: "info"
          volumeMounts:
            - name: config
              mountPath: /config
              readOnly: true
            - name: data
              mountPath: /data/ssmd
          resources:
            requests:
              cpu: 50m
              memory: 64Mi
            limits:
              cpu: 200m
              memory: 256Mi
      volumes:
        - name: config
          configMap:
            name: ssmd-archiver-config
        - name: data
          persistentVolumeClaim:
            claimName: ssmd-data
```

### PersistentVolumeClaim

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: ssmd-data
  namespace: ssmd
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 100Gi
```

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `RUST_LOG` | No | `info` | Log level |

### Verify Deployment

```bash
# Check pod
kubectl get pods -n ssmd -l app=ssmd-archiver

# Check logs
kubectl logs -n ssmd -l app=ssmd-archiver -f

# Check output files
kubectl exec -n ssmd deploy/ssmd-archiver -- ls -la /data/ssmd/kalshi/
```

---

## Production Architecture

For production, run both connector and archiver:
- **ssmd-connector**: Real-time streaming to NATS
- **ssmd-archiver**: Persists NATS data to disk for archival/replay

```
Kalshi WS → Connector → NATS JetStream → Archiver → JSONL.gz → GCS (cron)
```

## Network Policies

If using network policies, allow:

**ssmd-connector:**
- **Egress**: ssmd-connector → NATS (port 4222)
- **Egress**: ssmd-connector → Kalshi API (port 443, external)
- **Egress**: ssmd-connector → DNS (port 53)

**ssmd-archiver:**
- **Egress**: ssmd-archiver → NATS (port 4222)
- **Egress**: ssmd-archiver → DNS (port 53)

**ssmd-data:**
- **Ingress**: Clients → ssmd-data (port 8080)
- **Egress**: ssmd-data → PostgreSQL (port 5432, optional for secmaster)
- **Egress**: ssmd-data → DNS (port 53)

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `KALSHI_API_KEY` | Yes | - | Kalshi API key |
| `KALSHI_PRIVATE_KEY` | Yes | - | RSA private key (PEM) |
| `KALSHI_USE_DEMO` | No | `false` | Use demo API |
| `RUST_LOG` | No | `info` | Log level |

## Verify Deployment

```bash
# Check pods
kubectl get pods -n ssmd

# Check logs (NATS mode)
kubectl logs -n ssmd -l app=ssmd-connector-nats -f

# Check logs (File mode)
kubectl logs -n ssmd -l app=ssmd-connector-file -f

# Monitor NATS trades
nats sub -s nats://<nats-host>:4222 "prod.kalshi.trade.>"

# Check file output
kubectl exec -n ssmd deploy/ssmd-connector-file -- ls -la /data/
```

## Troubleshooting

### Connector not starting
```bash
kubectl describe pod -n ssmd -l app=ssmd-connector-nats
kubectl logs -n ssmd -l app=ssmd-connector-nats --previous
```

### NATS connection issues
```bash
# Test from pod
kubectl exec -n ssmd deploy/ssmd-connector-nats -- \
  nc -zv nats.nats.svc.cluster.local 4222
```

### No data publishing
```bash
# Check stream
nats stream info PROD_KALSHI -s nats://<nats-host>:4222

# Check connector logs for errors
kubectl logs -n ssmd -l app=ssmd-connector-nats | grep -i error
```

### File output not appearing
```bash
# Check disk space and permissions
kubectl exec -n ssmd deploy/ssmd-connector-file -- df -h /data
kubectl exec -n ssmd deploy/ssmd-connector-file -- ls -la /data/
```

---

## ssmd-data

HTTP API for serving archived market data. Used by ssmd-agent (local dev tool) and other clients.

### Container Image

```bash
# Build locally
docker build -f cmd/ssmd-data/Dockerfile -t ssmd-data:latest .

# Or use GHCR (tags trigger builds)
git tag v0.3.0
git push origin v0.3.0
# Image pushed to ghcr.io/<owner>/ssmd-data:0.3.0
```

### API Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check |
| `GET /version` | API version |
| `GET /datasets` | List available datasets |
| `GET /datasets/{feed}/{date}/sample` | Sample data with filters |
| `GET /datasets/{feed}/{date}/tickers` | List tickers in dataset |
| `GET /schema/{feed}/{type}` | Schema for message type |
| `GET /builders` | List available state builders |
| `GET /markets` | List markets (requires PostgreSQL) |
| `GET /markets/{ticker}` | Get market details (requires PostgreSQL) |
| `GET /fees` | Get fee tiers (requires PostgreSQL) |

> **Note:** Secmaster endpoints (`/markets`, `/fees`) return 503 if `DATABASE_URL` is not configured.

### Secret (API Key)

```bash
kubectl create secret generic ssmd-data-credentials \
  --namespace=ssmd \
  --from-literal=api-key="$(openssl rand -hex 32)"
```

### PostgreSQL Setup (Optional)

Required for secmaster endpoints (`/markets`, `/fees`). Skip if not using secmaster features.

```bash
# Create database secret
kubectl create secret generic ssmd-postgres-credentials \
  --namespace=ssmd \
  --from-literal=database-url="postgres://user:pass@postgres.ssmd.svc:5432/ssmd?sslmode=disable"

# Apply schema migration
kubectl run --rm -it psql-migrate --namespace=ssmd \
  --image=postgres:15-alpine \
  --env="PGPASSWORD=pass" \
  -- psql -h postgres.ssmd.svc -U user -d ssmd -f - < migrations/001_secmaster.sql
```

### Syncing Secmaster Data

Use the `ssmd secmaster sync` command to populate the database from Kalshi API:

```bash
# Full sync (all events and markets)
KALSHI_API_KEY=your-key \
KALSHI_PRIVATE_KEY_PATH=/path/to/key.pem \
DATABASE_URL="postgres://..." \
ssmd secmaster sync

# Incremental sync (only recent changes)
ssmd secmaster sync --incremental
```

Schedule periodic syncs via cron or Temporal workflow to keep data current.

### Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ssmd-data
  namespace: ssmd
spec:
  replicas: 1
  selector:
    matchLabels:
      app: ssmd-data
  template:
    metadata:
      labels:
        app: ssmd-data
    spec:
      containers:
        - name: data
          image: ghcr.io/<owner>/ssmd-data:0.3.0
          ports:
            - containerPort: 8080
              name: http
          env:
            - name: PORT
              value: "8080"
            - name: SSMD_DATA_PATH
              value: "/data/ssmd"
            - name: SSMD_API_KEY
              valueFrom:
                secretKeyRef:
                  name: ssmd-data-credentials
                  key: api-key
            # Optional: Enable secmaster endpoints
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: ssmd-postgres-credentials
                  key: database-url
                  optional: true
          volumeMounts:
            - name: data
              mountPath: /data/ssmd
              readOnly: true
          resources:
            requests:
              cpu: 50m
              memory: 64Mi
            limits:
              cpu: 200m
              memory: 256Mi
          livenessProbe:
            httpGet:
              path: /health
              port: http
            initialDelaySeconds: 5
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /health
              port: http
            initialDelaySeconds: 5
            periodSeconds: 5
      volumes:
        - name: data
          persistentVolumeClaim:
            claimName: ssmd-data
```

### Service

```yaml
apiVersion: v1
kind: Service
metadata:
  name: ssmd-data
  namespace: ssmd
spec:
  selector:
    app: ssmd-data
  ports:
    - port: 8080
      targetPort: http
      name: http
```

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `PORT` | No | `8080` | HTTP server port |
| `SSMD_DATA_PATH` | Yes | - | Path to archived data (local or `gs://bucket`) |
| `SSMD_API_KEY` | Yes | - | API key for authentication |
| `DATABASE_URL` | No | - | PostgreSQL connection string for secmaster |

### Verify Deployment

```bash
# Check pod
kubectl get pods -n ssmd -l app=ssmd-data

# Check health
kubectl exec -n ssmd deploy/ssmd-data -- curl -s localhost:8080/health

# Test API (get API key first)
API_KEY=$(kubectl get secret -n ssmd ssmd-data-credentials -o jsonpath='{.data.api-key}' | base64 -d)
kubectl exec -n ssmd deploy/ssmd-data -- \
  curl -s -H "X-API-Key: $API_KEY" localhost:8080/datasets

# Check logs
kubectl logs -n ssmd -l app=ssmd-data -f
```

### Exposing to Local Development

To use ssmd-agent from your laptop:

```bash
# Port forward (temporary)
kubectl port-forward -n ssmd svc/ssmd-data 8080:8080

# Or create an Ingress/LoadBalancer for persistent access
```
