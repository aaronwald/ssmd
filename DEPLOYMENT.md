# SSMD Deployment Guide

Deploy ssmd-connector to Kubernetes. Supports two output modes:

| Mode | Transport | Output | Use Case |
|------|-----------|--------|----------|
| **NATS** | `nats` | Cap'n Proto to JetStream | Real-time streaming |
| **File** | `memory` | Raw JSON to disk | Archival/replay |

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
git tag v0.1.0
git push origin v0.1.0
# Image pushed to ghcr.io/<owner>/ssmd-connector:0.1.0
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

## Deployment Option 1: NATS Streaming (Cap'n Proto)

Publishes normalized market data to NATS JetStream as Cap'n Proto messages.

**Subjects:**
- `{env}.{feed}.trade.{ticker}` - Trade executions
- `{env}.{feed}.ticker.{ticker}` - Price updates

### ConfigMap (NATS Mode)

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

### Deployment (NATS Mode)

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
          image: ghcr.io/<owner>/ssmd-connector:0.1.0
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

## Deployment Option 2: File Capture (Raw JSON)

Writes raw JSON messages to disk for archival and replay. Uses date-partitioned JSONL files.

**Output:** `/data/{date}/{feed}.jsonl`

### ConfigMap (File Mode)

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: ssmd-file-config
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

  environments/prod-file.yaml: |
    name: prod
    feed: kalshi
    schema: "trade:v1"
    transport:
      transport_type: memory
    storage:
      storage_type: local
      path: /data
```

### Deployment (File Mode)

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ssmd-connector-file
  namespace: ssmd
spec:
  replicas: 1
  selector:
    matchLabels:
      app: ssmd-connector-file
  template:
    metadata:
      labels:
        app: ssmd-connector-file
    spec:
      containers:
        - name: connector
          image: ghcr.io/<owner>/ssmd-connector:0.1.0
          args:
            - "--feed"
            - "/config/feeds/kalshi.yaml"
            - "--env"
            - "/config/environments/prod-file.yaml"
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
            - name: data
              mountPath: /data
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
            name: ssmd-file-config
        - name: data
          persistentVolumeClaim:
            claimName: ssmd-data
```

### PersistentVolumeClaim (File Mode)

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

---

## Running Both Modes

For production, run both deployments:
- **ssmd-connector-nats**: Real-time streaming to consumers
- **ssmd-connector-file**: Raw capture for archival/replay

Both connect to the same Kalshi feed but output to different destinations.

## Network Policies

If using network policies, allow:
- **Egress**: ssmd → NATS (port 4222) - NATS mode only
- **Egress**: ssmd → Kalshi API (port 443, external)
- **Egress**: ssmd → DNS (port 53)

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
