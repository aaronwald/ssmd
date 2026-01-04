# SSMD Operators

Kubernetes operators for managing SSMD market data pipeline components.

## Overview

The SSMD operator manages four Custom Resource types:

| CRD | Purpose | Creates |
|-----|---------|---------|
| **Connector** | WebSocket data ingestion | ConfigMap, Deployment |
| **Archiver** | NATS → JSONL.gz storage | ConfigMap, Deployment, PVC |
| **Signal** | Real-time signal computation | Deployment |
| **Notifier** | Alert routing to destinations | ConfigMap, Deployment |

## Installation

The operator is deployed via Flux GitOps in the `ssmd` namespace.

**Current version:** `ghcr.io/aaronwald/ssmd-operator:0.1.3`

### Prerequisites

- Kubernetes cluster with RBAC enabled
- `ghcr-secret` ImagePullSecret in `ssmd` namespace
- NATS JetStream available at `nats://nats.nats:4222`

## Custom Resources

### Connector

Manages WebSocket connections to market data feeds.

```yaml
apiVersion: ssmd.ssmd.io/v1alpha1
kind: Connector
metadata:
  name: kalshi-2026-01-04
  namespace: ssmd
spec:
  feed: kalshi                    # Feed name
  date: "2026-01-04"              # Trading day
  image: ghcr.io/aaronwald/ssmd-connector:0.4.7
  transport:
    type: nats
    url: nats://nats.nats:4222
    stream: PROD_KALSHI
    subjectPrefix: prod.kalshi
  secretRef:
    name: ssmd-kalshi-credentials
    apiKeyField: api-key
    privateKeyField: private-key
  resources:
    requests:
      cpu: 100m
      memory: 128Mi
    limits:
      cpu: 500m
      memory: 512Mi
```

**Status fields:**
- `phase`: Pending | Starting | Running | Failed | Terminated
- `deployment`: Name of created Deployment
- `conditions`: Ready condition with deployment status

**What the controller creates:**
1. ConfigMap with `feed.yaml` and `env.yaml` configuration
2. Deployment with config mounted at `/config`
3. Container args: `--feed /config/feed.yaml --env /config/env.yaml`

---

### Archiver

Archives NATS messages to local storage (and optionally GCS).

```yaml
apiVersion: ssmd.ssmd.io/v1alpha1
kind: Archiver
metadata:
  name: kalshi-2026-01-04
  namespace: ssmd
spec:
  feed: kalshi
  date: "2026-01-04"
  image: ghcr.io/aaronwald/ssmd-archiver:0.4.8
  source:
    stream: PROD_KALSHI
    url: nats://nats.nats:4222
    consumer: archiver-2026-01-04
  storage:
    local:
      path: /data/ssmd
      pvcName: ssmd-archiver-data    # Existing or new PVC
      pvcSize: 10Gi                   # Size if creating
    remote:                           # Optional GCS sync
      type: gcs
      bucket: ssmd-archive
      prefix: kalshi/2026/01/04
      secretRef: ssmd-gcs-credentials
  rotation:
    maxFileAge: "15m"
  sync:
    enabled: true
    onDelete: final                   # Sync before cleanup
  resources:
    requests:
      cpu: 100m
      memory: 256Mi
```

**Status fields:**
- `phase`: Pending | Starting | Running | Syncing | Failed | Terminated
- `deployment`: Name of created Deployment
- `conditions`: Ready, StorageHealthy

**What the controller creates:**
1. ConfigMap with `archiver.yaml` configuration
2. PVC if `pvcName` specified and doesn't exist
3. Deployment with config at `/config`, data at `/data`
4. Container args: `--config /config/archiver.yaml`

---

### Signal

Runs real-time signal computations on market data.

```yaml
apiVersion: ssmd.ssmd.io/v1alpha1
kind: Signal
metadata:
  name: kalshi-momentum
  namespace: ssmd
spec:
  signals:
    - momentum
    - volatility
    - spread-tracker
  image: ghcr.io/aaronwald/ssmd-signal-runner:0.1.1
  source:
    stream: PROD_KALSHI
    natsUrl: nats://nats.nats:4222
    categories:
      - Politics
    tickers:
      - KXBTC
  outputPrefix: signals.kalshi
  resources:
    requests:
      cpu: 100m
      memory: 128Mi
```

**Status fields:**
- `phase`: Pending | Running | Failed
- `deployment`: Name of created Deployment
- `signalMetrics`: Per-signal metrics (eventsProcessed, signalsGenerated)

**What the controller creates:**
1. Deployment with environment variables for configuration
2. Env vars: `SIGNALS`, `NATS_STREAM`, `NATS_URL`, `CATEGORIES`, `TICKERS`

---

### Notifier

Routes alerts and notifications to external destinations.

```yaml
apiVersion: ssmd.ssmd.io/v1alpha1
kind: Notifier
metadata:
  name: kalshi-alerts
  namespace: ssmd
spec:
  image: ghcr.io/aaronwald/ssmd-notifier:0.1.0
  source:
    subjects:
      - signals.kalshi.momentum.>
      - signals.kalshi.volatility.>
    natsUrl: nats://nats.nats:4222
  destinations:
    - name: slack-trading
      type: slack
      config:
        channel: "#trading-alerts"
      secretRef:
        name: slack-webhook
        key: url
    - name: email-ops
      type: email
      config:
        to: ops@example.com
  resources:
    requests:
      cpu: 50m
      memory: 64Mi
```

**Status fields:**
- `phase`: Pending | Running | Failed
- `destinationMetrics`: Per-destination delivery stats

**What the controller creates:**
1. ConfigMap with `destinations.json` configuration
2. Deployment with config mounted at `/config`
3. Secret volumes for each destination with `secretRef`

---

## Development

### Building

```bash
cd ssmd-operators

# Build locally
go build ./...

# Run tests
go test ./...

# Generate CRD manifests
make manifests

# Build container (via GitHub Actions)
git tag operator-v0.1.4
git push origin operator-v0.1.4
```

### Project Structure

```
ssmd-operators/
├── api/v1alpha1/           # CRD type definitions
│   ├── connector_types.go
│   ├── archiver_types.go
│   ├── signal_types.go
│   └── notifier_types.go
├── internal/controller/    # Reconciliation logic
│   ├── connector_controller.go
│   ├── archiver_controller.go
│   ├── signal_controller.go
│   └── notifier_controller.go
├── config/
│   ├── crd/                # Generated CRD YAML
│   ├── rbac/               # RBAC manifests
│   └── samples/            # Example CRs
└── cmd/main.go             # Operator entrypoint
```

### Deploying Updates

1. Make changes to controllers
2. Build and verify: `go build ./...`
3. Commit and tag: `git tag operator-v0.x.y`
4. Push tag: `git push origin operator-v0.x.y`
5. Wait for GitHub Actions build
6. Update varlab deployment image version
7. Push varlab and reconcile Flux

## RBAC

The operator requires these permissions:

| Resource | Verbs |
|----------|-------|
| connectors, archivers, signals, notifiers | get, list, watch, create, update, patch, delete |
| */status, */finalizers | get, update, patch |
| deployments | get, list, watch, create, update, patch, delete |
| configmaps | get, list, watch, create, update, patch, delete |
| persistentvolumeclaims | get, list, watch, create, update, patch, delete |
| secrets | get, list, watch |

## Troubleshooting

### Check operator logs

```bash
kubectl logs -n ssmd -l control-plane=controller-manager --tail=50
```

### Check CR status

```bash
kubectl get connector,archiver,signal,notifier -n ssmd
kubectl describe connector kalshi-2026-01-04 -n ssmd
```

### Common issues

**Pod stuck in ContainerCreating:**
- Check PVC availability (ReadWriteOnce can only attach to one node)
- Check imagePullSecrets exist

**Connector CrashLoopBackOff:**
- Check credentials secret exists with correct keys
- Check NATS connectivity

**Archiver not archiving:**
- Verify NATS stream and consumer exist
- Check storage path permissions

## License

Copyright 2026.

Licensed under the Apache License, Version 2.0.
