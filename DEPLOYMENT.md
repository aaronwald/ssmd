# SSMD Deployment

Kubernetes deployment via GitOps. Components are managed by ssmd-operator CRDs.

## Prerequisites

- Kubernetes cluster with NATS JetStream
- PostgreSQL (for secmaster)
- `kubeseal` CLI (for sealed secrets)

## Secrets

```bash
# Kalshi credentials
kubectl create secret generic ssmd-kalshi-credentials -n ssmd \
  --from-literal=api-key="$KALSHI_API_KEY" \
  --from-file=private-key=/path/to/key.pem

# ssmd-data API key
kubectl create secret generic ssmd-data-credentials -n ssmd \
  --from-literal=api-key="$(openssl rand -hex 32)"

# PostgreSQL (if not using operator-managed)
kubectl create secret generic ssmd-postgres-credentials -n ssmd \
  --from-literal=database-url="postgres://user:pass@host:5432/ssmd"
```

## Operator CRDs

The ssmd-operator manages pipeline components via Custom Resources.

### Connector

```yaml
apiVersion: ssmd.ssmd.io/v1alpha1
kind: Connector
metadata:
  name: kalshi-economics
  namespace: ssmd
spec:
  feed: kalshi
  categories: [Economics]
  image: ghcr.io/aaronwald/ssmd-connector:0.7.8
  replicas: 1
  shards: 3
  transport:
    type: nats
    url: nats://nats.nats.svc.cluster.local:4222
    stream: PROD_KALSHI_ECONOMICS
```

### Archiver

```yaml
apiVersion: ssmd.ssmd.io/v1alpha1
kind: Archiver
metadata:
  name: kalshi-archiver
  namespace: ssmd
spec:
  feed: kalshi
  image: ghcr.io/aaronwald/ssmd-archiver:0.7.8
  sources:
    - name: economics
      stream: PROD_KALSHI_ECONOMICS
      consumer: archiver-economics
      filter: "prod.kalshi.economics.json.>"
  storage:
    path: /data/ssmd
    rotation: 1h
  sync:
    enabled: true
    bucket: gs://ssmd-archives
```

### Signal

```yaml
apiVersion: ssmd.ssmd.io/v1alpha1
kind: Signal
metadata:
  name: volume-alert
  namespace: ssmd
spec:
  image: ghcr.io/aaronwald/ssmd-signal-runner:0.1.1
  signals: [volume-1m-30min]
  nats:
    url: nats://nats.nats.svc.cluster.local:4222
    inputStream: PROD_KALSHI
    outputStream: SIGNALS
```

## NATS Streams

Create streams before deploying connectors:

```bash
nats stream add PROD_KALSHI_ECONOMICS \
  --subjects "prod.kalshi.economics.json.>" \
  --retention limits --max-age 24h --storage file
```

## Images

| Component | Image | Tag Format |
|-----------|-------|------------|
| Connector/Archiver | `ghcr.io/aaronwald/ssmd-connector` | `v*` |
| Operator | `ghcr.io/aaronwald/ssmd-operator` | `operator-v*` |
| Signal Runner | `ghcr.io/aaronwald/ssmd-signal-runner` | `signal-runner-v*` |
| CLI | `ghcr.io/aaronwald/ssmd-cli-ts` | `cli-ts-v*` |
| Data API | `ghcr.io/aaronwald/ssmd-data-ts` | `data-ts-v*` |
| Notifier | `ghcr.io/aaronwald/ssmd-notifier` | `notifier-v*` |

Tag and push to trigger builds:

```bash
git tag v0.7.8 && git push origin v0.7.8
```

## Network Policies

Essential egress rules:

| Component | Destination | Port |
|-----------|-------------|------|
| Connector | NATS | 4222 |
| Connector | Kalshi API | 443 |
| Connector | ssmd-data-ts | 8080 |
| Archiver | NATS | 4222 |
| ssmd-data-ts | PostgreSQL | 5432 |

## Verify

```bash
# Check operator
kubectl get connectors,archivers,signals -n ssmd

# Check pods
kubectl get pods -n ssmd

# Monitor NATS
nats sub "prod.kalshi.>" -s nats://localhost:4222

# Check archives
kubectl exec -n ssmd deploy/ssmd-archiver -- ls -la /data/ssmd/kalshi/
```

## Troubleshooting

```bash
# Connector not publishing
kubectl logs -n ssmd -l app.kubernetes.io/name=ssmd-connector

# Check NATS stream
nats stream info PROD_KALSHI_ECONOMICS

# Operator issues
kubectl logs -n ssmd deploy/ssmd-operator
```
