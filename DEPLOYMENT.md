# SSMD Deployment Guide

Deploy ssmd-connector to Kubernetes (varlab homelab).

## Prerequisites

- Docker or Podman for building images
- `kubectl` configured for the target cluster
- `kubeseal` CLI for creating sealed secrets
- Access to GHCR (GitHub Container Registry)

## Build & Push Container Image

```bash
cd ssmd-rust

# Build the image
docker build -t ghcr.io/aaronwald/ssmd-connector:latest .

# Tag with version
docker tag ghcr.io/aaronwald/ssmd-connector:latest \
           ghcr.io/aaronwald/ssmd-connector:0.1.0

# Push to GHCR
docker push ghcr.io/aaronwald/ssmd-connector:latest
docker push ghcr.io/aaronwald/ssmd-connector:0.1.0
```

## Kubernetes Manifests

Manifests are in the varlab repo: `clusters/homelab/apps/ssmd/`

```
clusters/homelab/apps/ssmd/
├── kustomization.yaml
├── namespace.yaml
├── ghcr-secret.yaml          # Image pull secret
└── connector/
    ├── kustomization.yaml
    ├── configmap.yaml        # Feed + environment config
    ├── deployment.yaml
    ├── service.yaml
    └── sealed-secret.yaml    # Kalshi credentials
```

## Secrets Setup

### 1. GHCR Image Pull Secret

Copy from existing namespace or create new:

```bash
# Option A: Copy from teamwald
kubectl get secret ghcr-secret -n teamwald -o yaml | \
  sed 's/namespace: teamwald/namespace: ssmd/' | \
  kubectl apply -f -

# Option B: Create new
kubectl create secret docker-registry ghcr-secret \
  --namespace=ssmd \
  --docker-server=ghcr.io \
  --docker-username=<github-username> \
  --docker-password=<github-pat>
```

### 2. Kalshi API Credentials

Create sealed secret for Kalshi credentials:

```bash
# Create the secret (dry-run)
kubectl create secret generic ssmd-kalshi-credentials \
  --namespace=ssmd \
  --from-literal=api-key="$KALSHI_API_KEY" \
  --from-file=private-key=/path/to/kalshi-private-key.pem \
  --dry-run=client -o yaml > /tmp/kalshi-secret.yaml

# Seal it
kubeseal --format yaml < /tmp/kalshi-secret.yaml > \
  clusters/homelab/apps/ssmd/connector/sealed-secret.yaml

# Clean up
rm /tmp/kalshi-secret.yaml
```

## NATS Stream Configuration

Add ssmd stream to `clusters/homelab/infrastructure/nats/stream-config.yaml`:

```yaml
---
# In the ConfigMap data section, add:
  ssmd-trades.json: |
    {
      "name": "PROD_KALSHI",
      "description": "SSMD Kalshi market data",
      "subjects": ["prod.kalshi.>"],
      "retention": "limits",
      "max_age": 3600000000000,
      "storage": "file",
      "num_replicas": 1,
      "discard": "old"
    }
```

Update the Job to create the stream:

```bash
# Add to the init script
if nats stream info PROD_KALSHI -s nats://nats:4222 >/dev/null 2>&1; then
  echo "Stream PROD_KALSHI exists"
else
  nats stream add PROD_KALSHI \
    --subjects "prod.kalshi.>" \
    --description "SSMD Kalshi market data" \
    --retention limits \
    --max-age 1h \
    --storage file \
    --replicas 1 \
    --discard old \
    -s nats://nats:4222
fi
```

## Deploy with Flux

```bash
# Commit and push varlab changes
cd /workspaces/varlab
git add clusters/homelab/apps/ssmd
git add clusters/homelab/apps/ssmd-sync.yaml
git add clusters/homelab/infrastructure/network-policies/
git commit -m "feat: add ssmd-connector deployment"
git push

# Reconcile Flux
flux reconcile source git flux-system
flux reconcile kustomization infrastructure
flux reconcile kustomization ssmd
```

## Verify Deployment

```bash
# Check deployment status
kubectl get pods -n ssmd
kubectl logs -n ssmd -l app=ssmd-connector -f

# Check NATS connectivity
kubectl exec -n ssmd deploy/ssmd-connector -- \
  sh -c 'nc -zv nats.nats.svc.cluster.local 4222'

# Monitor trades on NATS
nats sub -s nats://10.20.0.100:4222 "prod.kalshi.trade.>"
```

## Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `KALSHI_API_KEY` | Yes | - | Kalshi API key |
| `KALSHI_PRIVATE_KEY` | Yes | - | RSA private key (PEM format) |
| `KALSHI_USE_DEMO` | No | `false` | Use demo API |
| `NATS_URL` | No | - | NATS server URL |
| `RUST_LOG` | No | `info` | Log level |

### Feed Configuration

Feed config is mounted from ConfigMap at `/config/feeds/kalshi.yaml`:

```yaml
name: kalshi
feed_type: websocket
versions:
  - version: "1.0"
    endpoint: wss://trading-api.kalshi.com/trade-api/ws/v2
```

### Environment Configuration

Environment config at `/config/environments/kalshi-prod.yaml`:

```yaml
name: prod
transport:
  transport_type: nats
  url: nats://nats.nats.svc.cluster.local:4222
storage:
  storage_type: file
  path: /data
```

## Troubleshooting

### Connector not starting

```bash
# Check pod status
kubectl describe pod -n ssmd -l app=ssmd-connector

# Check logs
kubectl logs -n ssmd -l app=ssmd-connector --previous
```

### NATS connection issues

```bash
# Verify network policy allows egress
kubectl get networkpolicy -n ssmd

# Test DNS resolution
kubectl exec -n ssmd deploy/ssmd-connector -- \
  nslookup nats.nats.svc.cluster.local
```

### No data on NATS

```bash
# Check stream exists
nats -s nats://10.20.0.100:4222 stream info PROD_KALSHI

# Check connector is publishing
kubectl logs -n ssmd -l app=ssmd-connector | grep -i publish
```

## Migration from tradfiportal

Once ssmd-connector is stable:

1. Update consumers to read from `prod.kalshi.trade.>` subjects
2. Disable tradfiportal collector: set `collector.replicaCount: 0`
3. Verify no data loss
4. Remove tradfiportal collector from values

## Image Versioning

Use semantic versioning for releases:

```bash
# Tag and push release
git tag v0.1.0
docker build -t ghcr.io/aaronwald/ssmd-connector:0.1.0 .
docker push ghcr.io/aaronwald/ssmd-connector:0.1.0

# Update deployment
# In clusters/homelab/apps/ssmd/connector/deployment.yaml:
# image: ghcr.io/aaronwald/ssmd-connector:0.1.0
```
