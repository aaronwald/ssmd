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
| ssmd-cli | Zig | Environment definition management |
| ssmd-connector | Zig | Kraken websocket ingestion + raw capture |
| ssmd-archiver | Zig | JetStream → Garage tiering |
| ssmd-gateway | Zig | WebSocket + REST API for agents |
| ssmd-reprocessor | Zig | Rebuild normalized data from raw |
| ssmd-tui | Zig | Terminal admin interface |

**Dependencies:**

| Service | Purpose |
|---------|---------|
| NATS + JetStream | Message streaming + persistence |
| Garage | S3-compatible object storage (open source) |
| SQLite | Entitlements and audit data |
| ArgoCD | GitOps deployment automation |

## Environment Definitions

A single source of truth for trading environment configuration. Removes operator error by validating everything before apply.

### Environment Spec

```yaml
# environments/prod.yaml
apiVersion: ssmd/v1
kind: Environment
metadata:
  name: prod

spec:
  feeds:
    kraken:
      enabled: true
      symbols:
        - name: BTC/USD
          internal: BTCUSD
          depth: l2
        - name: ETH/USD
          internal: ETHUSD
          depth: l2
        - name: SOL/USD
          internal: SOLUSD
          depth: l1

  storage:
    raw:
      bucket: ssmd-prod-raw
      retention_days: -1  # Forever
    normalized:
      bucket: ssmd-prod-normalized
      retention_days: 365

  jetstream:
    retention_hours: 24
    max_bytes: 10GB

  entitlements:
    - client_id: trading-bot-1
      name: "Main Trading Bot"
      type: trading
      feeds: [kraken]
      symbols: [BTCUSD, ETHUSD]

    - client_id: research-agent
      name: "Research Agent"
      type: non-display
      feeds: [kraken]
      symbols: ["*"]
```

### Multi-Environment Support

```
environments/
├── dev.yaml        # 2 symbols, 7-day retention, relaxed entitlements
├── staging.yaml    # 10 symbols, 30-day retention, prod-like entitlements
└── prod.yaml       # 50+ symbols, forever retention, strict entitlements
```

### CLI Tool (ssmd-cli)

**Environment management:**
```bash
ssmd env create dev --from-template minimal
ssmd env validate prod              # Check for errors before apply
ssmd env apply prod                 # Generate Helm values, commit to git
ssmd env diff dev prod              # Compare environments
ssmd env promote dev --to staging   # Copy with adjustments
ssmd env status prod                # Show what's deployed vs defined
```

**Feed management:**
```bash
ssmd feed list --env prod
ssmd feed add kraken AVAX/USD --depth l1 --env dev
ssmd feed remove kraken AVAX/USD --env dev
ssmd feed enable coinbase --env staging
```

**Symbol mapping:**
```bash
ssmd symbol map "BTC/USD" BTCUSD --feed kraken --env prod
ssmd symbol list --env prod
ssmd symbol validate --env prod     # Check all mappings resolve
```

**Client/entitlement management:**
```bash
ssmd client add research-bot --type non-display --env prod
ssmd client entitle research-bot --feed kraken --symbols "BTC*" --env prod
ssmd client revoke research-bot --feed kraken --env prod
ssmd client list --env prod
```

**Validation examples:**
```bash
$ ssmd env validate prod
✓ Feed 'kraken' configuration valid
✓ All symbol mappings resolve
✓ Storage buckets defined
✓ Entitlements reference valid clients
✓ No circular dependencies
Environment 'prod' is valid.

$ ssmd env validate broken
✗ Symbol 'XYZ/USD' not available on feed 'kraken'
✗ Client 'ghost-bot' referenced in entitlements but not defined
✗ Storage bucket 'ssmd-typo-raw' does not match naming convention
Environment 'broken' has 3 errors.
```

### GitOps Integration

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  ssmd-cli   │────▶│    Git      │────▶│   ArgoCD    │
│  (validate  │     │  (commit    │     │  (apply to  │
│   + generate)│     │   values)   │     │   cluster)  │
└─────────────┘     └─────────────┘     └─────────────┘
```

**Workflow:**
1. Operator runs `ssmd env apply prod`
2. CLI validates environment definition
3. CLI generates Helm values from environment spec
4. CLI commits to git (or outputs for manual commit)
5. ArgoCD detects change, syncs to cluster

**Generated files:**
```
charts/ssmd/
├── values.yaml                 # Defaults
├── values-dev.yaml             # Generated from environments/dev.yaml
├── values-staging.yaml         # Generated from environments/staging.yaml
└── values-prod.yaml            # Generated from environments/prod.yaml
```

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
- `ssmd-normalized` - Cap'n Proto files (versioned, keep forever)

## Reprocessor (Data Quality Iteration)

The Reprocessor rebuilds normalized data from raw when mappings change:

```
┌─────────────────────────────────────────────────────────┐
│                     Reprocessor                         │
│                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  Raw Reader  │─▶│  Normalizer  │─▶│ Garage Writer│  │
│  │   (Garage)   │  │  (new logic) │  │   (versioned)│  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
```

**Why this matters:**
- Raw data is immutable truth
- Normalization logic will have bugs, edge cases, improvements
- Must be able to regenerate any historical period with new logic

**Versioned output:**
```
/normalized/v1/kraken/trade/BTCUSD/2025/12/06/...   # Original
/normalized/v2/kraken/trade/BTCUSD/2025/12/06/...   # After mapping fix
```

**Workflow:**
1. Fix normalizer bug or improve mapping
2. Deploy new connector (live data uses new logic immediately)
3. Run reprocessor for affected date range
4. Validate new output vs old (diff sampling)
5. Update gateway to serve from new version
6. Prune old version after validation

**CLI interface:**
```bash
# Reprocess one day
ssmd-reprocessor --feed kraken --start 2025-12-01 --end 2025-12-01 --version v2

# Reprocess with parallelism
ssmd-reprocessor --feed kraken --start 2025-11-01 --end 2025-12-01 --version v2 --parallel 4

# Dry run (validate only, no write)
ssmd-reprocessor --feed kraken --start 2025-12-01 --end 2025-12-01 --version v2 --dry-run
```

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
│   ├── cli/
│   ├── connector/
│   ├── archiver/
│   ├── gateway/
│   ├── reprocessor/
│   └── tui/
├── proto/                           # Cap'n Proto schemas
├── environments/                    # Environment definitions
│   ├── dev.yaml
│   ├── staging.yaml
│   └── prod.yaml
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

### Phase 0: CLI & Environment Definitions (Build First)

Metadata support must come first. Remove chance of operator error.

- Build ssmd-cli skeleton in Zig
- Define environment YAML schema
- Implement `ssmd env create`, `validate`, `apply`
- Implement `ssmd feed add/remove/list`
- Implement `ssmd symbol map/list/validate`
- Generate Helm values from environment spec
- Validate: can create dev environment, generate valid Helm values

### Phase 1: Foundation

- Set up k3s cluster on homelab
- Deploy NATS + JetStream via Helm
- Deploy Garage via Helm
- Deploy ArgoCD, connect to repo
- Use ssmd-cli to create and apply dev environment
- Validate: publish/subscribe to NATS, write to Garage

### Phase 2: Core Ingestion

- Build Kraken connector in Zig
- Define Cap'n Proto schemas
- Implement websocket client + raw capture
- Connector reads symbol config from environment
- Publish normalized data to NATS
- Validate: see live data via `nats sub`

### Phase 3: Archival

- Build archiver (JetStream → Garage)
- Build reprocessor for data quality iteration
- Validate: data in Garage buckets, can read back, can reprocess

### Phase 4: Agent Access

- Build gateway (WebSocket + REST)
- Add entitlements (SQLite, API key validation)
- Implement `ssmd client add/entitle/revoke`
- Validate: Python notebook can subscribe, receive JSON

### Phase 5: Operations

- Build TUI
- Add Prometheus metrics
- Set up Grafana dashboards
- Add alerting rules

### Phase 6: Polish

- Audit reporting endpoints
- Documentation
- Implement `ssmd env promote` for staging→prod workflow
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

## Simplicity Metrics

"Simple" must be measurable. These are the operational simplicity targets for ssmd:

### Operational Targets

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Deploy from zero | ≤ 5 commands | Count commands in quickstart |
| Add new symbol | 1 config change, 0 restarts | Hot reload via NATS |
| Add new exchange | 1 new file + config | Connector per exchange |
| Recover from pod crash | Automatic, < 60s | k8s restart, verify with chaos testing |
| Recover from node failure | Automatic, < 5 min | Test by killing node |
| Upgrade component | Git push only | ArgoCD auto-sync |
| Rollback | Git revert only | ArgoCD auto-sync |
| Debug live issue | TUI + 2 commands max | `ssmd-tui` + `nats sub` |
| Check system health | 1 glance | TUI dashboard or Grafana |
| Backup/restore | ≤ 3 commands | Garage bucket sync |
| Access logs | 1 command | `kubectl logs` or Loki query |
| Cert rotation | Automatic | cert-manager |

### Data Quality Iteration Targets

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Deploy new normalizer | < 5 min from merge | ArgoCD sync time |
| Reprocess 1 day of raw data | < 1 hour | Time reprocessor run |
| Reprocess 1 month of raw data | < 24 hours | Parallelizable by day |
| Validate mapping change | < 5 min | Diff sample output old vs new |
| A/B test normalizer | 1 config flag | Run old + new in parallel |
| Rollback bad mapping | Git revert | Same as any rollback |

### Anti-Targets (Things We Refuse to Require)

- No SSH into nodes for normal operations
- No manual database migrations
- No coordination between component deploys
- No runbooks longer than 10 steps
- No manual certificate management
- No downtime for config changes
