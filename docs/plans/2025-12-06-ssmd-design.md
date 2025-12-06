# ssmd: Stupid Simple Market Data - Design Document

## Overview

ssmd is a homelab-friendly market data system built in Zig. It captures live crypto data, streams it for real-time consumption, and archives it for backtesting.

**Goals:**
- Simple enough to run on a homelab
- Simple enough to admin via TUI
- Simple enough for AI agents to access and reason about
- Cloud-first, GitOps-driven

**Non-goals:**
- Not a tickerplant (no complex routing logic)
- Not a shared library (though we provide client libraries)
- Not targeting ultra-low-latency HFT

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Kraken    │────▶│  Connector  │────▶│    NATS     │
│  WebSocket  │     │    (Zig)    │     │  JetStream  │
└──────┬──────┘     └─────────────┘     └──────┬──────┘
       │                                       │
       │            ┌─────────────┐            │
       └───────────▶│ Raw Capture │            │
                    │    (Zig)    │            │
                    └──────┬──────┘            │
                           │                   │
                           ▼                   │
                    ┌─────────────┐     ┌──────┴──────┐
                    │   Garage    │     │  Archiver   │
                    │  /raw/...   │     │   (Zig)     │
                    └─────────────┘     └──────┬──────┘
                                               │
                    ┌─────────────┐            │
                    │   Garage    │◀───────────┘
                    │ /normalized/│
                    └─────────────┘
```

**Components:**

| Component | Language | Purpose |
|-----------|----------|---------|
| ssmd-connector | Zig | Kraken websocket ingestion + raw capture |
| ssmd-archiver | Zig | JetStream → Garage tiering |
| ssmd-gateway | Zig | WebSocket + REST API for agents |
| ssmd-tui | Zig | Terminal admin interface |

**Dependencies:**

| Service | Purpose |
|---------|---------|
| NATS + JetStream | Message streaming + persistence |
| Garage | S3-compatible object storage (open source) |
| SQLite | Entitlements and audit data |
| ArgoCD | GitOps deployment automation |

## Data Schema (Cap'n Proto)

```capnp
@0xabcdef1234567890;

struct Timestamp {
  epochNanos @0 :UInt64;
}

struct Quote {
  symbol     @0 :Text;
  bidPrice   @1 :Float64;
  bidSize    @2 :Float64;
  askPrice   @3 :Float64;
  askSize    @4 :Float64;
  timestamp  @5 :Timestamp;
  exchange   @6 :Text;
}

struct Trade {
  symbol     @0 :Text;
  price      @1 :Float64;
  size       @2 :Float64;
  side       @3 :Side;
  timestamp  @4 :Timestamp;
  tradeId    @5 :Text;
  exchange   @6 :Text;
}

enum Side {
  buy  @0;
  sell @1;
}

struct OrderBookLevel {
  price @0 :Float64;
  size  @1 :Float64;
}

struct OrderBookSnapshot {
  symbol    @0 :Text;
  bids      @1 :List(OrderBookLevel);
  asks      @2 :List(OrderBookLevel);
  timestamp @3 :Timestamp;
  exchange  @4 :Text;
}
```

**NATS subject hierarchy:**
- `md.kraken.trade.BTCUSD` - trades for BTC/USD
- `md.kraken.quote.BTCUSD` - quotes
- `md.kraken.book.BTCUSD` - order book snapshots

## Connector Design

The Kraken Connector handles websocket ingestion and raw capture:

```
┌────────────────────────────────────────────────────────┐
│                   Kraken Connector                      │
│                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  WebSocket   │─▶│  Normalizer  │─▶│ NATS Publisher│  │
│  │    Client    │  │              │  │              │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
│         │                                              │
│         │              ┌──────────────┐                │
│         └─────────────▶│  Raw Writer  │────▶ Garage    │
│                        └──────────────┘                │
└────────────────────────────────────────────────────────┘
```

**Responsibilities:**
1. Maintain websocket connection to Kraken (reconnect on failure)
2. Fork incoming bytes: one path to raw storage, one path to normalizer
3. Normalize Kraken JSON → Cap'n Proto structs
4. Publish to NATS JetStream

**Raw storage format:**
```
/raw/kraken/2025/12/06/
  ├── 14/
  │   ├── 00-00.jsonl.zst   # Hour 14, minute 0
  │   ├── 00-01.jsonl.zst
  │   └── ...
```

Compressed JSONL files, partitioned by time.

**Configuration (TOML):**
```toml
[kraken]
ws_url = "wss://ws.kraken.com/v2"
symbols = ["BTC/USD", "ETH/USD"]

[nats]
url = "nats://localhost:4222"
stream = "marketdata"

[storage]
endpoint = "http://localhost:3900"
bucket = "ssmd-raw"
```

## Archiver & Storage Tiering

The Archiver moves normalized data from JetStream to Garage:

```
┌─────────────────────────────────────────────────────────┐
│                       Archiver                          │
│                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   JetStream  │─▶│   Batcher    │─▶│ Garage Writer│  │
│  │   Consumer   │  │   (time)     │  │              │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
```

**Behavior:**
1. Consumes from JetStream with a durable consumer
2. Batches messages by time window (1 minute)
3. Writes batched Cap'n Proto to Garage as compressed files
4. Acknowledges messages after successful write

**Normalized storage format:**
```
/normalized/kraken/trade/BTCUSD/
  └── 2025/12/06/
      ├── 14-00.capnp.zst
      ├── 14-01.capnp.zst
      └── ...
```

**JetStream retention:**
- Keep 24 hours in JetStream for live replay
- Archiver stays ~5 minutes behind live
- Data ages out of JetStream after archival

**Garage buckets:**
- `ssmd-raw` - Raw websocket data (keep forever)
- `ssmd-normalized` - Cap'n Proto files (keep forever)

## Agent Access

Three access patterns:

### 1. Direct NATS (lowest latency)

```
Agent ──▶ NATS Client ──▶ JetStream
```

- For Zig/Go clients that parse Cap'n Proto
- Subscribe to `md.kraken.trade.>` for all Kraken trades
- Replay from any point using JetStream consumer

### 2. WebSocket Gateway

```
Agent ◀──WS──▶ Gateway ◀──▶ NATS
            (JSON)    (Cap'n Proto)
```

- Translates Cap'n Proto → JSON
- Subscribe via: `ws://gateway/subscribe?symbols=BTCUSD,ETHUSD`
- Good for Python notebooks, LLM tool calls

### 3. REST API (historical queries)

```
GET /api/v1/trades?symbol=BTCUSD&start=2025-12-06T14:00:00Z&end=...
GET /api/v1/quotes?symbol=BTCUSD&last=100
GET /api/v1/symbols
```

- Queries Garage for archived data
- Returns JSON
- Good for backtesting, ad-hoc analysis

Single gateway binary handles both WebSocket and REST.

## Entitlements System

Exchange-compliant entitlements for future-proofing:

```
┌─────────────────────────────────────────────────────────┐
│                  Entitlements Service                   │
│                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   Policy     │  │   Session    │  │    Audit     │  │
│  │   Store      │  │   Tracker    │  │    Logger    │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
```

**Data model:**

```
Client
  ├── client_id (unique)
  ├── name
  ├── type: [display | non-display | trading]
  └── api_key

Entitlement
  ├── client_id
  ├── feed (e.g., "kraken", "nyse")
  ├── symbols: ["*" | list]
  ├── data_types: [trade, quote, book]
  └── expires_at

Session
  ├── client_id
  ├── connected_at
  ├── disconnected_at
  ├── device_id
  └── ip_address

UsageEvent
  ├── timestamp
  ├── client_id
  ├── feed
  ├── symbol
  ├── data_type
  ├── message_count
  └── display_type
```

**Enforcement:**
- Gateway validates API key + entitlements before allowing subscription
- Every message delivered increments usage counters
- Concurrent device limits enforced per client

**Audit endpoints:**
```
GET /api/v1/audit/usage?month=2025-12&feed=kraken
GET /api/v1/audit/peak-users?date=2025-12-06
GET /api/v1/audit/display-vs-nondisplay?month=2025-12
```

**Storage:** SQLite (simple, single file, can migrate to Postgres later)

## TUI Admin

Terminal interface for operating ssmd:

```
┌─ ssmd ──────────────────────────────────────────────────┐
│                                                         │
│  Status: ● Connected    Uptime: 4h 23m                  │
│                                                         │
│  ┌─ Feeds ────────────────────────────────────────────┐ │
│  │ ● kraken    12 symbols   1.2k msg/s   lag: 3ms     │ │
│  │ ○ coinbase  (not configured)                       │ │
│  └────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ Clients ──────────────────────────────────────────┐ │
│  │ agent-1     display     3 symbols    142 msg/s     │ │
│  │ backtest-2  non-display 12 symbols   0 msg/s       │ │
│  └────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ Storage ──────────────────────────────────────────┐ │
│  │ JetStream: 847 MB (18h retained)                   │ │
│  │ Garage:    12.4 GB raw / 8.2 GB normalized         │ │
│  └────────────────────────────────────────────────────┘ │
│                                                         │
│  [f]eeds  [c]lients  [s]torage  [l]ogs  [q]uit         │
└─────────────────────────────────────────────────────────┘
```

**Capabilities:**
- View live feed status and message rates
- Monitor connected clients and usage
- Check storage consumption
- Tail logs with filtering
- Manage entitlements (add/revoke clients)
- Trigger manual archival or replay

**Built with:** Zig + libvaxis

## Deployment & GitOps

### Repository Structure

```
ssmd/
├── src/                             # Zig source code
│   ├── connector/
│   ├── archiver/
│   ├── gateway/
│   └── tui/
├── proto/                           # Cap'n Proto schemas
├── charts/
│   └── ssmd/
│       ├── Chart.yaml
│       ├── values.yaml              # Defaults
│       ├── values-homelab.yaml      # Homelab overrides
│       └── templates/
│           ├── connector.yaml
│           ├── archiver.yaml
│           ├── gateway.yaml
│           ├── nats.yaml
│           ├── garage.yaml
│           ├── configmap.yaml
│           └── secrets.yaml
├── argocd/
│   ├── app-of-apps.yaml             # Parent app
│   ├── apps/
│   │   ├── ssmd-homelab.yaml
│   │   ├── nats.yaml
│   │   ├── garage.yaml
│   │   └── monitoring.yaml
│   └── projects/
│       └── ssmd.yaml
├── infra/
│   └── tofu/                        # OpenTofu for cloud
└── .github/
    └── workflows/
        ├── build.yml
        └── deploy.yml
```

### ArgoCD Application

```yaml
apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: ssmd
  namespace: argocd
spec:
  project: ssmd
  source:
    repoURL: https://github.com/you/ssmd
    path: charts/ssmd
    targetRevision: main
    helm:
      valueFiles:
        - values-homelab.yaml
  destination:
    server: https://kubernetes.default.svc
    namespace: ssmd
  syncPolicy:
    automated:
      prune: true
      selfHeal: true
```

### Workflow

1. Push config/code changes to `main`
2. ArgoCD detects drift
3. Auto-syncs (or manual approval if preferred)
4. Rollback via Git revert

## Observability

### Metrics (Prometheus)

```
# Connector
ssmd_connector_messages_received_total{feed="kraken", symbol="BTCUSD"}
ssmd_connector_websocket_reconnects_total{feed="kraken"}
ssmd_connector_lag_seconds{feed="kraken"}

# Gateway
ssmd_gateway_clients_connected{type="display"}
ssmd_gateway_messages_delivered_total{client="agent-1"}

# Archiver
ssmd_archiver_bytes_written_total{bucket="raw"}
ssmd_archiver_lag_seconds
```

### Logs

Structured JSON to stdout, collected with Loki.

### Healthchecks

```
GET /health         # Alive check
GET /health/ready   # Dependencies connected
```

### Alerting Rules

- Connector websocket disconnected > 30s
- Archiver lag > 5 minutes
- JetStream storage > 80% capacity
- Zero messages received in 60s during market hours

### Monitoring Stack (optional Helm dependencies)

```yaml
grafana:
  enabled: true
prometheus:
  enabled: true
loki:
  enabled: true
```

## Implementation Phases

### Phase 1: Foundation

- Set up k3s cluster on homelab
- Deploy NATS + JetStream via Helm
- Deploy Garage via Helm
- Deploy ArgoCD, connect to repo
- Validate: publish/subscribe to NATS, write to Garage

### Phase 2: Core Ingestion

- Build Kraken connector in Zig
- Define Cap'n Proto schemas
- Implement websocket client + raw capture
- Publish normalized data to NATS
- Validate: see live data via `nats sub`

### Phase 3: Archival

- Build archiver (JetStream → Garage)
- Validate: data in Garage buckets, can read back

### Phase 4: Agent Access

- Build gateway (WebSocket + REST)
- Add entitlements (SQLite, API key validation)
- Validate: Python notebook can subscribe, receive JSON

### Phase 5: Operations

- Build TUI
- Add Prometheus metrics
- Set up Grafana dashboards
- Add alerting rules

### Phase 6: Polish

- Audit reporting endpoints
- Documentation
- Add second exchange to validate normalization layer

## Resource Estimates (Homelab)

### Compute

| Component | CPU | Memory | Notes |
|-----------|-----|--------|-------|
| ssmd-connector | 0.1 core | 64 MB | Zig binary, minimal footprint |
| ssmd-archiver | 0.1 core | 128 MB | Batch writes, mostly idle |
| ssmd-gateway | 0.2 core | 128 MB | Scales with connected clients |
| NATS + JetStream | 0.5 core | 512 MB | Depends on message volume |
| Garage (3-node min) | 0.3 core × 3 | 256 MB × 3 | Distributed, needs 3 nodes |
| ArgoCD | 0.3 core | 512 MB | Can share with other workloads |
| Prometheus | 0.2 core | 512 MB | Depends on retention |
| Grafana | 0.1 core | 256 MB | Mostly idle |
| Loki | 0.2 core | 256 MB | Depends on log volume |

**Minimum total:** ~2 cores, 3 GB RAM (tight, single-node k3s)

**Recommended:** 4 cores, 8 GB RAM (comfortable headroom)

### Storage

| Data Type | Daily Volume | Monthly Volume | Notes |
|-----------|--------------|----------------|-------|
| Raw Kraken (2 symbols) | ~500 MB | ~15 GB | Compressed JSONL |
| Raw Kraken (50 symbols) | ~5 GB | ~150 GB | Scales linearly |
| Normalized | ~60% of raw | ~60% of raw | Cap'n Proto + zstd |
| JetStream | 2-5 GB | N/A | Rolling 24h window |
| Prometheus metrics | ~100 MB | ~3 GB | 15-day retention |

**Minimum storage:** 100 GB SSD (gets you started)

**Recommended:** 500 GB - 1 TB SSD (room to grow, add symbols)

### Network

- Kraken websocket: ~1-10 Mbps depending on symbols/depth
- Internal cluster traffic: ~2x ingestion rate
- Modest homelab internet connection sufficient

### Example Homelab Configurations

**Minimal (Raspberry Pi 4 8GB or similar):**
- Single k3s node
- 2-3 symbols
- 30-day retention
- No monitoring stack (use TUI only)

**Comfortable (Mini PC / NUC):**
- Single k3s node, 4+ cores, 16 GB RAM
- 20+ symbols
- 90-day retention
- Full monitoring stack

**Proper (3-node cluster):**
- 3× mini PCs or old laptops
- Garage distributed properly
- High availability
- Multi-month retention

## Technology Summary

| Concern | Choice |
|---------|--------|
| Language | Zig |
| Serialization | Cap'n Proto |
| Messaging | NATS + JetStream |
| Object Storage | Garage |
| Entitlements DB | SQLite |
| Container Orchestration | Kubernetes (k3s) |
| Package Management | Helm |
| GitOps | ArgoCD |
| Monitoring | Prometheus + Grafana + Loki |
| TUI Framework | libvaxis |
| Initial Exchange | Kraken |
