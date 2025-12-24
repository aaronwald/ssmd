# SSMD Deployment Guide

Deploy ssmd-connector to Kubernetes.

## Prerequisites

- Kubernetes cluster with NATS JetStream
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

### ConfigMap (Feed + Environment)

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: ssmd-config
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

  environments/prod.yaml: |
    name: prod
    transport:
      transport_type: nats
      url: nats://nats.nats.svc.cluster.local:4222
    storage:
      storage_type: file
      path: /data
```

### Secret (Kalshi Credentials)

```bash
kubectl create secret generic ssmd-kalshi-credentials \
  --namespace=ssmd \
  --from-literal=api-key="$KALSHI_API_KEY" \
  --from-file=private-key=/path/to/kalshi-private-key.pem
```

### Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ssmd-connector
  namespace: ssmd
spec:
  replicas: 1
  selector:
    matchLabels:
      app: ssmd-connector
  template:
    metadata:
      labels:
        app: ssmd-connector
    spec:
      containers:
        - name: connector
          image: ghcr.io/<owner>/ssmd-connector:0.1.0
          args:
            - "--feed"
            - "/config/feeds/kalshi.yaml"
            - "--env"
            - "/config/environments/prod.yaml"
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
              value: "info,ssmd_connector=debug"
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
            name: ssmd-config
```

### Service

```yaml
apiVersion: v1
kind: Service
metadata:
  name: ssmd-connector
  namespace: ssmd
spec:
  type: ClusterIP
  ports:
    - port: 8080
      targetPort: health
      name: health
  selector:
    app: ssmd-connector
```

## NATS JetStream Stream

Create a stream for market data:

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

## Network Policies

If using network policies, allow:
- **Egress**: ssmd → NATS (port 4222)
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

# Check logs
kubectl logs -n ssmd -l app=ssmd-connector -f

# Monitor NATS trades
nats sub -s nats://<nats-host>:4222 "prod.kalshi.trade.>"
```

## Troubleshooting

### Connector not starting
```bash
kubectl describe pod -n ssmd -l app=ssmd-connector
kubectl logs -n ssmd -l app=ssmd-connector --previous
```

### NATS connection issues
```bash
# Test from pod
kubectl exec -n ssmd deploy/ssmd-connector -- \
  nc -zv <nats-service> 4222
```

### No data publishing
```bash
# Check stream
nats stream info PROD_KALSHI -s nats://<nats-host>:4222

# Check connector logs for errors
kubectl logs -n ssmd -l app=ssmd-connector | grep -i error
```
