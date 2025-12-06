# ssmd: Stupid Simple Market Data - Design Document

> **Note:** This document has grown comprehensive. Consider splitting into:
> - `design-overview.md` - Architecture, components, data flow
> - `design-operations.md` - CLI, TUI, deployment, observability
> - `design-security.md` - Auth, RBAC, network security, audit
> - `design-data.md` - Schema, sharding, storage, enrichment

## Overview

ssmd is a homelab-friendly market data system. It captures live crypto data, streams it for real-time consumption, and archives it for backtesting.

**Language strategy:** Go for tooling (CLI, TUI, Temporal workers), Zig or C++ for data path (connector, archiver, gateway).

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
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Kraken    │────▶│ Connector   │────▶│    NATS     │────▶│  Archiver   │
│  WebSocket  │     │ (per shard) │     │  JetStream  │     │ (per shard) │
└──────┬──────┘     └─────────────┘     └──────┬──────┘     └──────┬──────┘
       │                                       │                   │
       │            ┌─────────────┐            │                   │
       └───────────▶│ Raw Capture │            │                   │
                    │ (per shard) │            ▼                   ▼
                    └──────┬──────┘     ┌─────────────┐     ┌─────────────┐
                           │            │   NATS      │     │   Garage    │
                           │            │  Mirroring  │     │ /normalized/│
                           ▼            └──────┬──────┘     └─────────────┘
                    ┌─────────────┐            │
                    │   Garage    │            ▼
                    │  /raw/...   │     ┌─────────────┐
                    └─────────────┘     │  Gateway    │◀──▶ Clients
                                        │  (unified)  │
                                        └─────────────┘
```

Connectors and archivers run per-shard (e.g., `ssmd-connector-tier1`, `ssmd-archiver-tier1`). NATS mirroring merges internal shard subjects into unified client-facing subjects. See [Sharding](#sharding-connectors-and-collectors) for details.

**Components:**

| Component | Language | Purpose |
|-----------|----------|---------|
| ssmd-cli | Go | Environment definition management |
| ssmd-tui | Go | Terminal admin interface |
| ssmd-worker | Go | Temporal workflow worker + Lua transforms |
| ssmd-connector | Zig/C++ | Market data ingestion (live, replay, reprocess modes) |
| ssmd-archiver | Zig/C++ | JetStream → Garage tiering (live path) |
| ssmd-gateway | Zig/C++ | WebSocket + REST API for agents |
| ssmd-mcp | Go | MCP server for AI agent tool access |

**Language split rationale:**
- **Go for tooling** - Better ecosystem for CLI (cobra), YAML parsing, rapid iteration
- **Zig for data path** - Performance, small binaries, no GC pauses

**Existing assets:**
- `libechidna` - C++ library with io_uring, SSL/TLS, WebSocket, HTTP/2
- `olalla` - C++ io_uring-based Kraken connector using libechidna

**Open decision: Connector implementation**

| Option | Pros | Cons |
|--------|------|------|
| Use existing C++ (olalla/libechidna) | Already working, battle-tested io_uring | Mixed language codebase |
| Wrap C++ from Zig | Zig ergonomics + proven io_uring impl | FFI complexity |
| Port to Zig | Pure Zig, simpler build | Rewriting working code |
| C++ connector, Zig elsewhere | Best of both, isolate C++ to hot path | Two build systems |

Decision can be deferred. Zig has direct syscall access (`std.os.linux.io_uring`), so porting is feasible if desired.

**Dependencies:**

| Service | Purpose |
|---------|---------|
| NATS + JetStream | Message streaming + persistence (initial transport) |
| Garage | S3-compatible object storage (open source) |
| PostgreSQL | Entitlements, audit data, secmaster |
| ArgoCD | GitOps deployment automation |
| Temporal | Job scheduling with market calendars |

**Transport abstraction:** The messaging layer (NATS) is abstracted behind a transport interface. This allows future migration to alternatives like Aeron for lower latency or Chronicle for on-prem deployments. Connectors and archivers depend on the transport interface, not NATS directly.

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
  # Symbol definitions with attributes for sharding
  symbols:
    BTCUSD:
      feed: kraken
      external: BTC/USD
      tier: 1
      depth: l2
    ETHUSD:
      feed: kraken
      external: ETH/USD
      tier: 1
      depth: l2
    SOLUSD:
      feed: kraken
      external: SOL/USD
      tier: 2
      depth: l1

  # Shard definitions with selectors
  shards:
    tier1:
      selector:
        tier: 1
      resources:
        cpu: "0.5"
        memory: "256Mi"
    tier2:
      selector:
        tier: 2
      resources:
        cpu: "0.2"
        memory: "128Mi"

  # Transport configuration (pluggable)
  transport:
    type: nats  # nats | aeron | chronicle
    subjects:
      internal: "internal.{shard}.{feed}.{type}.{symbol}"
      client: "md.{feed}.{type}.{symbol}"

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
✓ All symbols have exactly one shard
✓ All shards have matching symbols
✓ Storage buckets defined
✓ Entitlements reference valid clients
Environment 'prod' is valid.

$ ssmd env validate broken
✗ Symbol 'XYZ/USD' not available on feed 'kraken'
✗ Symbol 'AVAXUSD' has no matching shard
✗ Shard 'tier3' matches no symbols
✗ Client 'ghost-bot' referenced in entitlements but not defined
Environment 'broken' has 4 errors.
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

### Configuration Hot-Reload

Add/remove symbols without restarts:

1. **Config stored in NATS KV** - Environment spec synced to NATS key-value store
2. **Components watch for changes** - Connector subscribes to config changes
3. **Apply without restart** - Add symbol = subscribe to new websocket channel

```
ssmd env apply prod
       │
       ▼
┌─────────────────┐     ┌─────────────────┐
│  Generate Helm  │────▶│  NATS KV Store  │
│  values + sync  │     │  (config.prod)  │
└─────────────────┘     └────────┬────────┘
                                 │
                                 ▼
                        ┌─────────────────┐
                        │  Connector      │
                        │  watches config │
                        │  adds symbols   │
                        └─────────────────┘
```

Some changes (e.g., new exchange, schema version) still require deployment.

## Security Master

A security master (secmaster) is the authoritative source for instrument identification and metadata. It maps between identifiers and tracks changes over time.

### Design Principles

- **FIGI as canonical identifier** - Open standard, free, covers all asset classes
- **Temporal by default** - All mappings have effective dates; query as-of any point in time
- **Additive history** - Never delete, only add new versions with new effective dates
- **Exchange symbols are mappings** - External symbols (BTC/USD on Kraken) map to internal identifiers

### Identifier Hierarchy

```
FIGI (canonical, permanent)
  └── Internal Symbol (ssmd identifier, e.g., BTCUSD)
        └── Exchange Symbols (per feed, per venue)
              ├── kraken: BTC/USD
              ├── coinbase: BTC-USD
              └── binance: BTCUSDT
```

### Temporal Model

Every mapping has an `effective_from` date. Queries specify an as-of date:

```
┌────────────────────────────────────────────────────────────┐
│  Symbol: ETHUSD                                            │
├──────────────┬─────────────────────────────────────────────┤
│ effective_from │ kraken_symbol                              │
├──────────────┼─────────────────────────────────────────────┤
│ 2020-01-01   │ ETH/USD                                     │
│ 2023-06-15   │ ETHUSD  (hypothetical symbol change)        │
└──────────────┴─────────────────────────────────────────────┘

Query: "What was ETHUSD's Kraken symbol on 2022-03-01?"
Answer: ETH/USD (effective_from 2020-01-01 was active)
```

### Schema

```yaml
# secmaster/instruments.yaml
instruments:
  BTCUSD:
    figi: BBG00JR8TZ84  # Bitcoin USD (example)
    name: Bitcoin / US Dollar
    asset_class: crypto
    base: BTC
    quote: USD

  ETHUSD:
    figi: BBG00QFKJ5L8  # Ethereum USD (example)
    name: Ethereum / US Dollar
    asset_class: crypto
    base: ETH
    quote: USD

# secmaster/mappings/kraken.yaml
feed: kraken
mappings:
  - symbol: BTCUSD
    external: BTC/USD
    effective_from: 2020-01-01

  - symbol: ETHUSD
    external: ETH/USD
    effective_from: 2020-01-01

# secmaster/mappings/coinbase.yaml
feed: coinbase
mappings:
  - symbol: BTCUSD
    external: BTC-USD
    effective_from: 2021-01-01
```

### Corporate Actions & Changes

The secmaster tracks instrument lifecycle events:

| Event | How Handled |
|-------|-------------|
| Symbol rename | New mapping with new effective_from |
| Merger/acquisition | New instrument, old instrument marked inactive |
| Delisting | Instrument marked inactive with effective date |
| Exchange adds symbol | New mapping added |
| Exchange changes symbol | New mapping version |

### CLI Commands

```bash
# Instrument management
ssmd secmaster add SOLUSD --figi BBG00EXAMPLE --asset-class crypto
ssmd secmaster show BTCUSD                    # Current state
ssmd secmaster show BTCUSD --as-of 2023-01-01 # Historical state
ssmd secmaster history BTCUSD                 # All versions

# Mapping management
ssmd secmaster map BTCUSD --feed kraken --external "BTC/USD" --effective-from 2020-01-01
ssmd secmaster mappings BTCUSD                # Show all feed mappings
ssmd secmaster mappings BTCUSD --as-of 2022-06-01

# Lookup
ssmd secmaster lookup --feed kraken --external "BTC/USD"  # Find internal symbol
ssmd secmaster lookup --figi BBG00JR8TZ84                 # Find by FIGI

# FIGI integration
ssmd secmaster figi-lookup AAPL              # Query OpenFIGI API
ssmd secmaster figi-sync --feed kraken       # Sync FIGIs for all symbols
```

### Environment Integration

The environment spec references the secmaster:

```yaml
# environments/prod.yaml
spec:
  secmaster:
    source: ./secmaster          # Path to secmaster files
    as_of: latest                # or specific date for reproducibility

  symbols:
    BTCUSD:
      tier: 1
      depth: l2
    ETHUSD:
      tier: 1
      depth: l2
    # Feed-specific external symbols resolved from secmaster
```

### API Endpoints

```
GET /api/v1/secmaster/instruments
GET /api/v1/secmaster/instruments/{symbol}
GET /api/v1/secmaster/instruments/{symbol}?as_of=2023-01-01
GET /api/v1/secmaster/instruments/{symbol}/history
GET /api/v1/secmaster/mappings/{feed}
GET /api/v1/secmaster/lookup?figi={figi}
GET /api/v1/secmaster/lookup?feed={feed}&external={symbol}
```

### Storage

**Two-tier storage model:**

1. **Git (YAML files)** - Source of truth for stable instruments
   - Version control and audit trail
   - GitOps workflow (PR to add/change instruments)
   - Loaded on startup

2. **NATS KV (runtime)** - Dynamic updates for intraday changes
   - Hot additions without restart
   - Synced back to Git periodically or on-demand
   - Components watch for changes

Runtime lookup via PostgreSQL (fast queries, populated from both sources).

### Dynamic Instrument Creation

Prediction markets, new token listings, and other instruments can appear intraday. The secmaster supports hot updates:

```bash
# Add instrument immediately (no restart required)
ssmd secmaster add TRUMPWIN2024 \
  --asset-class prediction \
  --feed polymarket \
  --external "Will Trump win 2024?" \
  --effective-from now \
  --dynamic   # Writes to NATS KV, not Git

# Connector picks up new symbol automatically
# Later, sync to Git for permanence
ssmd secmaster sync-to-git --pending
```

**Flow:**

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ ssmd secmaster   │────▶│  NATS KV    │────▶│ Connectors  │
│ add --dynamic│     │  (immediate) │     │ (watch for  │
└─────────────┘     └─────────────┘     │  changes)   │
                           │            └─────────────┘
                           ▼
                    ┌─────────────┐
                    │  Git sync   │ (periodic or manual)
                    │  (durable)  │
                    └─────────────┘
```

**Use cases:**
| Scenario | Approach |
|----------|----------|
| Polymarket creates new market | `ssmd secmaster add --dynamic` |
| Exchange lists new token | `ssmd secmaster add --dynamic` |
| Planned instrument addition | Git PR workflow |
| Corporate action (known in advance) | Git PR with future effective_from |

**Connector behavior:**
- Watches NATS KV for secmaster changes
- New instrument appears → subscribes to feed if shard selector matches
- Instrument deactivated → unsubscribes
- No restart required for dynamic changes

### OpenFIGI Integration

For instruments with FIGIs, we can enrich from the OpenFIGI API:

```bash
# Bulk lookup
ssmd secmaster figi-enrich --feed kraken

# Validates our mappings against OpenFIGI
ssmd secmaster figi-validate
```

Note: Crypto instruments may not have FIGIs assigned. FIGI coverage is strongest for equities, bonds, and listed derivatives.

## Capture Provenance

Every recorded message must capture where and when it was received, with clock quality metadata.

### Why This Matters

- **Latency analysis** - Distance from exchange affects timestamp meaning
- **Backtesting accuracy** - Know the precision of your historical timestamps
- **Regulatory compliance** - MiFID II requires timestamp accuracy reporting
- **Debugging** - Was the clock drifting? Was NTP healthy?
- **Multi-location** - Data from different capture points must be reconcilable

### Capture Metadata

Every raw message includes:

```yaml
capture:
  # Location
  location_id: us-east-1a        # Unique capture point identifier
  datacenter: aws-us-east-1      # Datacenter/cloud region
  host: connector-tier1-abc123   # Specific host
  proximity: remote              # exchange-colo | same-metro | remote

  # Clock
  clock_source: ntp              # ntp | ptp | hardware | gps
  clock_sync_status: synced      # synced | unsynced | degraded
  clock_offset_us: 1200          # Estimated offset from true time (microseconds)
  clock_last_sync: 2025-12-06T14:23:00Z

  # Software
  connector_version: v1.2.3      # Git tag or semver
  connector_commit: abc1234      # Short git SHA
  connector_build: 2025-12-05T10:00:00Z

  # Timestamps
  capture_time_ns: 1733497380123456789   # When we received it (our clock)
  exchange_time_ns: 1733497380123000000  # Exchange timestamp (if provided)
```

### Clock Quality Levels

| Source | Typical Accuracy | Use Case |
|--------|------------------|----------|
| `hardware` | < 1 μs | Exchange colo, FPGA timestamping |
| `ptp` | 1-100 μs | Datacenter with PTP infrastructure |
| `gps` | 1-10 μs | Dedicated GPS receiver |
| `ntp` | 1-50 ms | Cloud VMs, homelab |
| `unsynced` | Unknown | Clock sync failed, flag data |

### Location Registry

```yaml
# config/locations.yaml
locations:
  us-east-1a:
    datacenter: aws-us-east-1
    region: us-east
    cloud: aws
    proximity:
      kraken: remote        # ~20ms to Kraken
      coinbase: same-metro  # ~2ms to Coinbase
    clock:
      source: ntp
      ntp_servers: [169.254.169.123]  # AWS time sync
      expected_accuracy_ms: 10

  equinix-ny5:
    datacenter: equinix-ny5
    region: us-east
    cloud: none
    proximity:
      nyse: exchange-colo
      nasdaq: exchange-colo
    clock:
      source: ptp
      ptp_domain: 0
      expected_accuracy_us: 50
```

### Schema Integration

```capnp
struct CaptureMetadata {
  locationId      @0 :Text;
  clockSource     @1 :ClockSource;
  clockSyncStatus @2 :ClockSyncStatus;
  clockOffsetUs   @3 :Int32;        # Signed, can be negative
  captureTimeNs   @4 :UInt64;
  exchangeTimeNs  @5 :UInt64;       # 0 if not provided by exchange

  # Software lineage
  softwareVersion @6 :Text;         # e.g., "v1.2.3"
  softwareCommit  @7 :Text;         # Short git SHA
  softwareBuild   @8 :UInt64;       # Build timestamp (unix epoch)
}

enum ClockSource {
  hardware @0;
  ptp @1;
  gps @2;
  ntp @3;
  unsynced @4;
}

enum ClockSyncStatus {
  synced @0;
  degraded @1;    # Sync working but accuracy reduced
  unsynced @2;    # Sync failed
}
```

### Raw Storage Format

Raw files include capture metadata in header:

```
/raw/kraken/2025/12/06/14/00-00.jsonl.zst
```

File header (first line):
```json
{"_capture": {"location_id": "us-east-1a", "clock_source": "ntp", "clock_offset_us": 1200, "connector_version": "v1.2.3", "connector_commit": "abc1234", ...}}
```

### Clock Monitoring & Skew Detection

**Metrics:**
```
# Per-location clock health
ssmd_clock_offset_us{location="us-east-1a", source="ntp"}
ssmd_clock_sync_status{location="us-east-1a"}  # 1=synced, 0=unsynced
ssmd_clock_last_sync_seconds{location="us-east-1a"}

# Clock skew between locations
ssmd_clock_skew_us{location_a="us-east-1a", location_b="us-west-2b"}

# Skew between our clock and exchange timestamps
ssmd_exchange_clock_skew_us{location="us-east-1a", feed="kraken"}

# Clock jump detection (sudden changes)
ssmd_clock_jump_detected{location="us-east-1a"}
ssmd_clock_jump_magnitude_us{location="us-east-1a"}
```

**Skew calculation:**
- Compare `capture_time_ns` vs `exchange_time_ns` for each message
- Track rolling statistics (mean, stddev, percentiles)
- Detect anomalies (sudden shifts, gradual drift)

```
Exchange says: 14:23:00.000
We received:   14:23:00.025
Skew:          +25ms (we're behind, or network latency)

If skew suddenly changes from +25ms to +500ms:
  → Clock jump detected, or exchange issue
```

**Multi-location skew:**
```
Location A capture_time: 14:23:00.100
Location B capture_time: 14:23:00.095
Skew A↔B: 5ms

If skew grows over time → one location drifting
If skew is stable → network latency difference, not clock issue
```

**Alerts:**
```yaml
alerts:
  - name: clock-unsynced
    condition: ssmd_clock_sync_status == 0
    for: 1m
    severity: critical
    annotations:
      description: "Clock unsynced at {{ $labels.location }} - timestamps unreliable"

  - name: clock-drift-high
    condition: abs(ssmd_clock_offset_us) > 10000  # >10ms
    for: 5m
    severity: warning

  - name: clock-skew-growing
    condition: |
      abs(delta(ssmd_clock_skew_us[10m])) > 1000  # Skew growing >1ms/10min
    severity: warning
    annotations:
      description: "Clock skew between {{ $labels.location_a }} and {{ $labels.location_b }} is growing"

  - name: exchange-clock-skew-anomaly
    condition: |
      abs(ssmd_exchange_clock_skew_us - avg_over_time(ssmd_exchange_clock_skew_us[1h])) > 50000
    severity: warning
    annotations:
      description: "Exchange clock skew changed significantly for {{ $labels.feed }}"

  - name: clock-jump-detected
    condition: ssmd_clock_jump_detected == 1
    severity: critical
    annotations:
      description: "Clock jump of {{ $labels.magnitude }}μs detected at {{ $labels.location }}"
```

**Clock skew dashboard (TUI):**
```
┌─ Clock Health ──────────────────────────────────────────┐
│                                                         │
│  LOCATION      SOURCE  OFFSET   SKEW→EXCH  STATUS      │
│  us-east-1a    ntp     +1.2ms   +25ms      ● synced    │
│  us-west-2b    ntp     -0.8ms   +28ms      ● synced    │
│  equinix-ny5   ptp     +12μs    +2ms       ● synced    │
│                                                         │
│  Cross-location skew:                                   │
│  us-east-1a ↔ us-west-2b:  2.0ms (stable)              │
│  us-east-1a ↔ equinix-ny5: 1.1ms (stable)              │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### CLI Commands

```bash
# View clock status across locations
ssmd clock status
# Output:
#   LOCATION      SOURCE  STATUS   OFFSET    LAST SYNC
#   us-east-1a    ntp     synced   +1.2ms    2s ago
#   us-west-2b    ntp     synced   -0.8ms    5s ago
#   equinix-ny5   ptp     synced   +12μs     <1s ago

# View clock skew between locations
ssmd clock skew
# Output:
#   LOCATION A     LOCATION B      SKEW     TREND
#   us-east-1a     us-west-2b      2.0ms    stable
#   us-east-1a     equinix-ny5     1.1ms    stable
#   us-west-2b     equinix-ny5     0.9ms    stable

# View skew to exchange clocks
ssmd clock skew --exchange
# Output:
#   LOCATION      FEED      SKEW     TREND
#   us-east-1a    kraken    +25ms    stable
#   us-east-1a    coinbase  +18ms    stable
#   equinix-ny5   kraken    +2ms     stable

# Historical skew analysis
ssmd clock skew-history us-east-1a --hours 24

# Check clock health for a location
ssmd clock check us-east-1a

# Force NTP sync (if permitted)
ssmd clock sync us-east-1a

# Investigate clock jump
ssmd clock jumps --location us-east-1a --days 7
```

### Querying by Capture Quality

When querying historical data, filter by clock quality:

```bash
# Only use data with good clock sync
ssmd query trades BTCUSD \
  --start 2025-12-01 \
  --end 2025-12-05 \
  --clock-source ptp,gps,hardware \
  --clock-status synced
```

API equivalent:
```
GET /api/v1/trades?symbol=BTCUSD&clock_source=ptp,gps&clock_status=synced
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

Internal (shard-aware, used by connectors/archivers):
- `internal.tier1.kraken.trade.BTCUSD` - tier1 shard trades
- `internal.tier2.kraken.trade.SOLUSD` - tier2 shard trades

Client-facing (unified via NATS mirroring):
- `md.kraken.trade.BTCUSD` - trades for BTC/USD
- `md.kraken.quote.BTCUSD` - quotes
- `md.kraken.book.BTCUSD` - order book snapshots

Clients subscribe to `md.*` subjects and are unaware of sharding.

## Connector Design

Each connector instance handles a shard's symbols. Multiple connector deployments run in parallel (e.g., `ssmd-connector-tier1`, `ssmd-connector-tier2`).

**Input abstraction:** The connector has pluggable input sources. Same normalization logic runs against live or replayed data:

```
┌─────────────────────────────────────────────────────────────┐
│              Kraken Connector (per shard)                   │
│                                                             │
│  ┌──────────────┐                                           │
│  │  WebSocket   │──┐                                        │
│  │  (live)      │  │                                        │
│  └──────────────┘  │   ┌──────────────┐  ┌──────────────┐  │
│                    ├──▶│  Normalizer  │─▶│    Output    │  │
│  ┌──────────────┐  │   │  + Enrichers │  │   Adapter    │  │
│  │ Garage/Raw   │──┘   └──────────────┘  └──────────────┘  │
│  │  (replay)    │                              │            │
│  └──────────────┘                              ▼            │
│                                    ┌───────────────────┐    │
│                                    │ NATS (live)       │    │
│                                    │ Garage (replay)   │    │
│                                    └───────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Modes:**
| Mode | Input | Output | Use Case |
|------|-------|--------|----------|
| `live` | WebSocket | NATS + Garage/raw | Normal operation |
| `replay` | Garage/raw | NATS | Backfill, testing |
| `reprocess` | Garage/raw | Garage/normalized | Rebuild with new version |

**Responsibilities:**
1. Read `SHARD_ID` from environment, resolve to symbol list
2. Connect to input source (websocket or Garage reader)
3. Subscribe only to symbols assigned to this shard
4. In live mode: fork incoming bytes to raw storage
5. Normalize exchange JSON → Cap'n Proto structs
6. Run enrichment chain
7. Publish to output (NATS subjects or Garage files)

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

Each archiver instance handles a shard's data. Multiple archiver deployments run in parallel, mirroring the connector sharding (e.g., `ssmd-archiver-tier1`, `ssmd-archiver-tier2`).

```
┌─────────────────────────────────────────────────────────┐
│                  Archiver (per shard)                   │
│                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   JetStream  │─▶│   Batcher    │─▶│ Garage Writer│  │
│  │   Consumer   │  │   (time)     │  │              │  │
│  │ (internal.   │  └──────────────┘  └──────────────┘  │
│  │  {shard}.*)  │                                      │
│  └──────────────┘                                      │
└─────────────────────────────────────────────────────────┘
```

**Behavior:**
1. Read `SHARD_ID` from environment
2. Consumes from internal shard subjects (`internal.{shard}.*`) with a durable consumer
3. Batches messages by time window (1 minute)
4. Writes batched Cap'n Proto to Garage as compressed files
5. Acknowledges messages after successful write

**Normalized storage format:**
```
/normalized/v1/kraken/trade/BTCUSD/
  └── 2025/12/06/
      ├── 14-00.capnp.zst
      ├── 14-01.capnp.zst
      └── ...
```

Version prefix (`v1`, `v2`, etc.) allows reprocessing to create new versions without overwriting.

**Archive file metadata:**

Every archive file includes a header with processing lineage:

```json
{
  "_lineage": {
    "schema_version": "v1",
    "processor": "archiver",
    "processor_version": "v1.2.3",
    "processor_commit": "abc1234",
    "processor_build": "2025-12-05T10:00:00Z",
    "processing_time": "2025-12-06T14:05:00Z",
    "source": "internal.tier1.kraken.trade.BTCUSD",
    "message_count": 12847,
    "time_range": {
      "start": "2025-12-06T14:00:00Z",
      "end": "2025-12-06T14:01:00Z"
    }
  }
}
```

`processor` is `archiver` (live path) or `connector` (reprocess mode).

This enables:
- **Reproducibility** - Know exactly what code processed the data
- **Reprocessing** - Re-run with newer connector when bugs are fixed
- **Auditing** - Trace any data quality issue to specific software version
- **Comparison** - Run old vs new version and diff outputs

**JetStream retention:**
- Keep 24 hours in JetStream for live replay
- Archiver stays ~5 minutes behind live
- Data ages out of JetStream after archival

**Garage buckets:**
- `ssmd-raw` - Raw websocket data (keep forever)
- `ssmd-normalized` - Cap'n Proto files (versioned, keep forever)

**Object versioning:**

Garage supports S3-compatible object versioning. Enable on all buckets:

```bash
# Enable versioning on buckets
ssmd storage versioning enable ssmd-raw
ssmd storage versioning enable ssmd-normalized

# Or via garage-admin
garage bucket allow --read --write --owner ssmd-raw
garage bucket website --allow ssmd-raw  # if needed
```

```yaml
# config/storage.yaml
buckets:
  ssmd-raw:
    versioning: enabled
    lifecycle:
      # Keep all versions (raw is source of truth)
      noncurrent_versions: keep_all

  ssmd-normalized:
    versioning: enabled
    lifecycle:
      # Keep last 3 versions of normalized data
      noncurrent_versions: 3
      # Delete noncurrent after 90 days
      noncurrent_expiration_days: 90
```

**Why versioning matters:**
- **Accidental overwrites** - Can recover previous version
- **Reprocessing safety** - Old version preserved until new is verified
- **Audit trail** - See history of changes to any object
- **Rollback** - Quick recovery if new archiver has bugs

**CLI for version management:**
```bash
# List versions of an object
ssmd storage versions /normalized/v1/kraken/trade/BTCUSD/2025/12/06/14-00.capnp.zst

# Restore previous version
ssmd storage restore /normalized/v1/kraken/trade/BTCUSD/2025/12/06/14-00.capnp.zst --version-id abc123

# Delete specific version
ssmd storage delete /normalized/v1/kraken/trade/BTCUSD/2025/12/06/14-00.capnp.zst --version-id abc123

# Prune old versions (keep last N)
ssmd storage prune ssmd-normalized --keep 2 --older-than 30d --dry-run
```

## Sharding Connectors and Collectors

As ssmd scales to more symbols and higher throughput, a single connector instance becomes a bottleneck. High-volume symbols like BTC/USD can overwhelm a single process. Sharding partitions work across multiple connector and collector instances.

**Goals:**
- Scale horizontally by partitioning symbols across shards
- Keep sharding as an internal implementation detail - clients don't see shards
- Drive all sharding configuration from metadata (environment definition)
- Support multi-tenancy through environment isolation
- Enable fast resharding via blue-green deployments

**Design principles:**
- Stupid simple: explicit configuration over clever automation
- Fail fast: validation catches all errors before apply
- No side effects: changing one shard doesn't affect others
- GitOps native: resharding is a new environment, not in-place migration

### Sharding Model

Symbols carry metadata attributes. Shards define selectors that match those attributes.

```yaml
symbols:
  BTCUSD:
    tier: 1
    asset_class: crypto
  ETHUSD:
    tier: 1
    asset_class: crypto
  SOLUSD:
    tier: 2
    asset_class: crypto

shards:
  tier1:
    selector:
      tier: 1
  tier2:
    selector:
      tier: 2
```

In this example:
- `tier1` shard handles BTCUSD and ETHUSD (both have `tier: 1`)
- `tier2` shard handles SOLUSD (has `tier: 2`)

Selectors can match multiple attributes:
```yaml
shards:
  high-volume-crypto:
    selector:
      tier: 1
      asset_class: crypto
```

### Shard Identity

Each shard is a separate Kubernetes Deployment. The shard ID is passed as an environment variable:

```yaml
# ssmd-connector-tier1 deployment
spec:
  template:
    spec:
      containers:
        - name: connector
          env:
            - name: SHARD_ID
              value: tier1
```

On startup, the connector:
1. Reads `SHARD_ID` from environment
2. Queries metadata for shard definition
3. Resolves selector to list of symbols
4. Subscribes to exchange feeds for those symbols only

**Why explicit env var (not StatefulSet ordinals):**
- Clear: deployment name tells you what shard it is
- Decoupled: can add/remove shards without renumbering
- Debuggable: `kubectl get pods` shows `ssmd-connector-tier1-xxx`

### Mirrored Sharding

Connectors and collectors shard identically. A `tier1` connector's output goes to a `tier1` collector.

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ Connector-tier1 │────▶│ NATS (internal) │────▶│ Collector-tier1 │
└─────────────────┘     └─────────────────┘     └─────────────────┘

┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ Connector-tier2 │────▶│ NATS (internal) │────▶│ Collector-tier2 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

**Why mirrored:**
- Simple: no cross-shard coordination
- Scalable: add capacity by scaling both together
- Isolated: tier1 issues don't affect tier2

### NATS Subject Architecture

Two subject hierarchies: internal (shard-aware) and client-facing (shard-agnostic).

**Internal subjects (connectors → collectors):**
```
internal.{shard}.{feed}.{type}.{symbol}
internal.tier1.kraken.trade.BTCUSD
internal.tier2.kraken.trade.SOLUSD
```

**Client-facing subjects (for subscribers):**
```
md.{feed}.{type}.{symbol}
md.kraken.trade.BTCUSD
md.kraken.trade.SOLUSD
```

**NATS subject mirroring** routes internal subjects to client-facing subjects:
```
internal.tier1.kraken.trade.BTCUSD  →  md.kraken.trade.BTCUSD
internal.tier2.kraken.trade.SOLUSD  →  md.kraken.trade.SOLUSD
```

**Why this separation:**
- Clients don't know about shards (implementation detail)
- Clients subscribe to `md.kraken.trade.>` and get all symbols
- Resharding doesn't change client subscriptions
- Collectors subscribe to `internal.tier1.>` for their shard only

### Environment Definition with Sharding

The environment file is the single source of truth for sharding configuration. See [Environment Spec](#environment-spec) for the complete schema showing symbols with attributes, shard definitions with selectors and resources, and NATS subject patterns.

Everything in one file: symbol attributes, shard definitions, resource allocations, subject patterns. No external registries or separate Helm overrides for sharding.

### Multi-Tenancy

**Tenant = Environment.** Each tenant gets their own environment file, shards, and NATS subjects.

```
environments/
  customer-abc.yaml   # ABC's symbols, shards, subjects
  customer-xyz.yaml   # XYZ's symbols, shards, subjects
```

**Why not shared-environment multi-tenancy:**
- Simpler: no access control logic within shards
- Isolated: one tenant's issues can't affect another
- Auditable: clear separation for compliance

### Sharding Validation Rules

`ssmd env validate` enforces strict rules. All violations are errors (fail fast).

| Rule | Error Message |
|------|---------------|
| Symbol with no matching shard | `Symbol BTCUSD has no matching shard` |
| Shard with no matching symbols | `Shard tier3 matches no symbols` |
| Symbol matches multiple shards | `Symbol BTCUSD matches shards: tier1, high-volume` |
| Selector references unknown attribute | `Shard tier1 references unknown attribute 'priority'` |

**No warnings, no soft failures.** Validation either passes or fails.

### Resharding Operations

Resharding uses blue-green deployment. No in-place migration.

```bash
# 1. Create new environment from existing
ssmd env create prod-v2 --from prod

# 2. Edit sharding (e.g., split tier1 into tier1a and tier1b)
vim environments/prod-v2.yaml

# 3. Validate
ssmd env validate prod-v2

# 4. Deploy new environment
ssmd env apply prod-v2

# 5. Verify, cut over clients, decommission old
ssmd env delete prod
```

**Why blue-green:**
- Safe: old environment runs until new one is verified
- Reversible: if new sharding has issues, old environment still works
- GitOps native: each environment is a versioned configuration

### Sharding CLI Commands

```bash
# List shards and their selectors
ssmd shard list --env prod

# Show shard health and throughput
ssmd shard status --env prod

# Show symbols assigned to a shard
ssmd shard symbols tier1 --env prod

# Show which shard owns a symbol
ssmd symbol show BTCUSD --env prod
```

### Sharding Observability

**Metrics (add `shard` label):**
```
ssmd_connector_messages_total{shard="tier1", feed="kraken", symbol="BTCUSD"}
ssmd_connector_lag_seconds{shard="tier1", feed="kraken"}
ssmd_shard_symbols_count{shard="tier1", env="prod"}
```

**Alerts:**
- Shard lag > 5 seconds
- Shard throughput dropped > 50% vs baseline
- Shard imbalance (one shard handling >80% of volume)

**TUI:**
```
┌─ Shards ────────────────────────────────────────────┐
│ ● tier1   3 symbols   45k msg/s   lag: 2ms         │
│ ● tier2   47 symbols  12k msg/s   lag: 5ms         │
└─────────────────────────────────────────────────────┘
```

## Reprocessing (Data Quality Iteration)

Reprocessing uses the connector in `reprocess` mode - same normalization logic, reading from raw storage instead of websocket. See [Connector Design](#connector-design) for mode details.

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
3. Run `ssmd reprocess` for affected date range (Temporal spawns connector in reprocess mode)
4. Validate new output vs old (diff sampling)
5. Update gateway to serve from new version
6. Prune old version after validation

**CLI:** See [Temporal CLI Integration](#cli-integration) for `ssmd reprocess` commands.

## Agent Access

Four access patterns:

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

### 4. MCP Server (AI Agent Native)

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  AI Agent   │◀───▶│ MCP Server  │◀───▶│   Gateway   │
│ (Claude,    │     │ (ssmd-mcp)  │     │   + NATS    │
│  GPT, etc)  │     │             │     │   + Garage  │
└─────────────┘     └─────────────┘     └─────────────┘
```

MCP (Model Context Protocol) gives AI agents native tool access to ssmd:

**Tools exposed:**
```yaml
tools:
  # Market data
  - name: subscribe
    description: Subscribe to real-time market data
    parameters:
      symbols: [BTCUSD, ETHUSD]
      feed: kraken

  - name: get_trades
    description: Get historical trades
    parameters:
      symbol: BTCUSD
      start: 2025-12-06T14:00:00Z
      end: 2025-12-06T15:00:00Z

  - name: get_quote
    description: Get current best bid/ask
    parameters:
      symbol: BTCUSD

  # Instrument discovery
  - name: list_symbols
    description: List available symbols
    parameters:
      feed: kraken
      asset_class: crypto

  - name: get_instrument
    description: Get instrument details (FIGI, tick size, etc)
    parameters:
      symbol: BTCUSD

  # Data quality
  - name: report_issue
    description: Report data quality issue
    parameters:
      type: bad_tick | stale_quote | missing_data
      symbol: BTCUSD
      evidence: {...}

  # System status
  - name: get_feed_status
    description: Check feed health
    parameters:
      feed: kraken

  - name: get_latency
    description: Get current latency metrics
```

**Resources exposed:**
```yaml
resources:
  - uri: ssmd://feeds
    description: List of available feeds

  - uri: ssmd://feeds/kraken/symbols
    description: Symbols available on Kraken

  - uri: ssmd://instruments/{figi}
    description: Instrument by FIGI

  - uri: ssmd://status
    description: System health status
```

**Implementation:**
- `ssmd-mcp` is a Go binary using the MCP SDK
- Connects to gateway via internal API (not public endpoints)
- Handles streaming via MCP's server-sent events
- Runs on edge (same node as gateway) for low latency

**Deployment:**
```yaml
# environments/prod.yaml
mcp:
  enabled: true
  port: 8080
  auth:
    type: api_key  # Same entitlements as REST/WS
  rate_limit:
    requests_per_minute: 100
```

**CLI:**
```bash
ssmd mcp status                    # MCP server health
ssmd mcp clients                   # Connected AI agents
ssmd mcp tools                     # List exposed tools
```

### 5. Customer Transforms (Lua)

Customers may need data in different formats. Gateway supports Lua transforms:

```lua
-- transforms/customer-abc.lua
function transform(msg)
  return {
    ticker = msg.symbol:gsub("/", "-"),
    px = msg.price,
    sz = msg.size,
    ts = msg.timestamp.epochNanos / 1e9,
    src = msg.exchange:upper()
  }
end
```

**Registration:**
```bash
ssmd transform add customer-abc --file transforms/customer-abc.lua --env prod
ssmd transform test customer-abc --feed kraken --symbol BTCUSD  # Test with live data
ssmd transform list --env prod
```

**Usage:**
```
GET /api/v1/trades?symbol=BTCUSD&transform=customer-abc
WS: {"subscribe": "BTCUSD", "transform": "customer-abc"}
```

**Lifecycle:**
1. Customer requests custom format
2. Create Lua transform, test with real data
3. Deploy (stored in environment spec)
4. Once stable, promote to first-class code in next release
5. Remove Lua version after code ships

**Implementation:** Transforms run in ssmd-worker (Go + gopher-lua). Gateway routes transform requests to worker via NATS. This keeps the hot path (gateway) in Zig/C++ while allowing flexible Lua scripting in Go.

### 6. Enrichment Pipeline

Internal enrichment adds value to raw data before it reaches clients. Unlike customer transforms (output formatting), enrichment modifies the canonical normalized data.

```
┌─────────┐    ┌─────────────┐    ┌─────────────┐    ┌──────────┐
│   Raw   │───▶│  Normalizer │───▶│  Enrichers  │───▶│Normalized│
│  Data   │    │             │    │  (chain)    │    │  Output  │
└─────────┘    └─────────────┘    └─────────────┘    └──────────┘
                                         │
                      ┌──────────────────┼──────────────────┐
                      ▼                  ▼                  ▼
               ┌───────────┐      ┌───────────┐      ┌───────────┐
               │ Secmaster │      │  Derived  │      │  Quality  │
               │   Join    │      │  Fields   │      │   Flags   │
               └───────────┘      └───────────┘      └───────────┘
```

**Enricher types:**
| Type | Purpose | Example |
|------|---------|---------|
| Secmaster join | Add reference data | FIGI, asset class, lot size |
| Derived fields | Calculate values | mid price, spread, VWAP |
| Quality flags | Mark anomalies | stale quote, crossed market |
| Cross-feed | Join related data | Index vs constituents |

**Configuration:**
```yaml
# config/enrichment.yaml
enrichers:
  - name: secmaster-join
    type: reference_join
    source: secmaster
    fields: [figi, asset_class, tick_size]

  - name: derived-fields
    type: calculation
    fields:
      mid: "(bid + ask) / 2"
      spread: "ask - bid"
      spread_bps: "(ask - bid) / mid * 10000"

  - name: quality-flags
    type: quality
    rules:
      - name: stale_quote
        condition: "age_ms > 5000"
      - name: crossed_market
        condition: "bid > ask"
      - name: wide_spread
        condition: "spread_bps > 100"
```

**CLI:**
```bash
ssmd enricher list
ssmd enricher add derived-fields --config enrichment.yaml
ssmd enricher test derived-fields --feed kraken --symbol BTCUSD
ssmd enricher disable quality-flags  # Temporarily disable
```

**Implementation:** Enrichers run in the archiver pipeline (Zig/C++). Chain is defined in config; each enricher is a function that takes a message and returns an enriched message. Enricher version is recorded in archive lineage.

### 7. Agent Feedback API

Agents can report data quality issues and feature requests programmatically.

**Structured feedback (known issue types):**
```bash
POST /api/v1/feedback
{
  "type": "data_quality",
  "subtype": "bad_tick",
  "feed": "kraken",
  "symbol": "BTCUSD",
  "timestamp": "2025-12-06T14:23:00Z",
  "evidence": {
    "price": 150000,
    "prev_price": 50000,
    "jump_pct": 200
  },
  "description": "Price jumped 200% in 1 tick"
}
```

**Natural language (novel issues):**
```bash
POST /api/v1/feedback
{
  "type": "unknown",
  "description": "Order book depth seems inconsistent with trade volume. Seeing 10x more trades than available liquidity suggests.",
  "context": {
    "agent": "liquidity-analyzer",
    "task": "market-making-signal",
    "data_window": "2025-12-06T14:00:00Z/2025-12-06T15:00:00Z"
  }
}
```

**Known issue types:**
| Type | Subtype | Description |
|------|---------|-------------|
| data_quality | bad_tick | Obvious erroneous price |
| data_quality | gap | Missing data in sequence |
| data_quality | duplicate | Same record multiple times |
| data_quality | stale | Data not updating |
| data_quality | schema | Unexpected field/format |
| feature_request | - | Agent needs new capability |
| bug | - | System not behaving as documented |
| unknown | - | Agent can't categorize |

**Feedback flow:**
```
Agent submits feedback
        │
        ▼
┌─────────────────┐
│   Validation    │ Deduplicate, check if known issue
└────────┬────────┘
         │
    ┌────┴────┐
    │         │
    ▼         ▼
Known?    Novel?
    │         │
    ▼         ▼
Aggregate   Create
+ alert     Linear issue
```

**CLI for reviewing feedback:**
```bash
ssmd feedback list --status open
ssmd feedback resolve FB-123 --action fixed --release v1.2
ssmd feedback stats --days 30  # Trends
```

**Metrics:**
```
ssmd_feedback_total{type="data_quality", subtype="bad_tick"}
ssmd_feedback_resolved_total{action="fixed"}
ssmd_feedback_time_to_resolve_seconds
```

## Consumer Backpressure & Defensive Design

The system must protect itself from misbehaving or slow clients. One slow consumer should never impact other consumers or the data pipeline.

### Pull Consumers Everywhere

All internal consumers use JetStream pull consumers (not push):

```yaml
consumer:
  type: pull              # Client controls pace
  max_ack_pending: 1000   # Pause delivery if too many unacked
  ack_wait: 30s           # Redeliver if not acked
  max_deliver: 3          # Give up after 3 attempts
```

**Why pull:**
- Natural backpressure - consumer requests messages at its pace
- No server-side buffering per consumer
- JetStream stream is the buffer, shared by all consumers

### Gateway Client Protection

The gateway must protect itself from slow WebSocket clients:

```
┌─────────────────────────────────────────────────────────┐
│                       Gateway                            │
│                                                         │
│  ┌──────────────┐     ┌──────────────┐                 │
│  │ NATS Pull    │────▶│ Per-Client   │────▶ WebSocket  │
│  │ Consumer     │     │ Buffer       │                 │
│  └──────────────┘     │ (bounded)    │                 │
│                       └──────┬───────┘                 │
│                              │                         │
│                       Buffer full?                     │
│                       ├── Drop oldest (conflation)     │
│                       ├── Drop client                  │
│                       └── Alert                        │
└─────────────────────────────────────────────────────────┘
```

**Per-client settings (configurable in entitlements):**

```yaml
entitlements:
  - client_id: trading-bot-1
    buffer_size: 10000        # Messages before action
    buffer_policy: drop_oldest # drop_oldest | disconnect | block
    max_lag_seconds: 30       # Alert if behind by this much
    rate_limit: 50000         # Max msg/sec delivered
```

**Buffer policies:**
| Policy | Behavior | Use Case |
|--------|----------|----------|
| `drop_oldest` | Conflate - keep latest per symbol | Display clients, dashboards |
| `disconnect` | Kill connection when buffer full | Misbehaving clients |
| `block` | Stop reading from NATS (backpressure) | Internal consumers only |

### Slow Consumer Detection

**Metrics:**
```
# Per-client lag
ssmd_gateway_client_lag_seconds{client="trading-bot-1"}
ssmd_gateway_client_buffer_usage{client="trading-bot-1"}
ssmd_gateway_client_drops_total{client="trading-bot-1", reason="buffer_full"}

# Aggregate
ssmd_gateway_slow_consumers_count
ssmd_gateway_disconnects_total{reason="slow_consumer"}
```

**Alerts:**
```yaml
alerts:
  - name: client-falling-behind
    condition: ssmd_gateway_client_lag_seconds > 10
    for: 1m
    severity: warning
    annotations:
      description: "Client {{ $labels.client }} is {{ $value }}s behind"

  - name: client-dropping-messages
    condition: rate(ssmd_gateway_client_drops_total[5m]) > 100
    severity: warning

  - name: slow-consumer-epidemic
    condition: ssmd_gateway_slow_consumers_count > 5
    severity: critical
    annotations:
      description: "Multiple slow consumers - possible system issue"
```

### CLI Tools for Managing Clients

```bash
# View client health
ssmd client status --env prod
# Output:
#   CLIENT         LAG     BUFFER   MSG/S   STATUS
#   trading-bot-1  0.1s    12%      4500    healthy
#   research-bot   45.2s   98%      200     slow
#   broken-client  120s    100%     0       dropping

# View slow consumers
ssmd client slow --env prod
ssmd client slow --env prod --lag-threshold 10s

# Disconnect a misbehaving client
ssmd client disconnect research-bot --env prod --reason "buffer overflow"

# Temporarily block a client (entitlement still valid, just can't connect)
ssmd client block broken-client --env prod --duration 1h --reason "investigation"
ssmd client unblock broken-client --env prod

# Rate limit a client
ssmd client rate-limit research-bot --env prod --max-rate 10000

# View client history (connections, disconnects, issues)
ssmd client history research-bot --env prod --days 7
```

### TUI Slow Consumer View

```
┌─ Clients ───────────────────────────────────────────────┐
│                                                         │
│  CLIENT          TYPE        LAG    BUFFER  MSG/S  ⚠   │
│  trading-bot-1   trading     0.1s   12%     4500       │
│  research-bot    non-display 45.2s  98%     200    ⚠   │
│  broken-client   display     120s   FULL    0      ✗   │
│                                                         │
│  [d]isconnect  [b]lock  [r]ate-limit  [h]istory        │
└─────────────────────────────────────────────────────────┘
```

### Defensive Defaults

| Setting | Default | Rationale |
|---------|---------|-----------|
| Client buffer size | 10,000 msgs | ~10 seconds at 1k msg/s |
| Default buffer policy | `drop_oldest` | Prefer availability over completeness for display |
| Max lag before alert | 30s | Enough time for brief hiccups |
| Max lag before disconnect | 5 min | Client is clearly broken |
| Rate limit (default) | None | Trust entitled clients initially |
| Reconnect backoff | Exponential, max 60s | Prevent reconnect storms |

### Protection Layers

```
Layer 1: Entitlements     - Client not entitled? Reject connection
Layer 2: Rate limiting    - Too many requests? Throttle
Layer 3: Buffer policy    - Buffer full? Drop/disconnect per policy
Layer 4: Lag monitoring   - Falling behind? Alert operators
Layer 5: Manual override  - Operator disconnects/blocks client
```

### Audit Trail

All client management actions logged:

```
ssmd audit client-actions --env prod --days 7
# Output:
#   TIMESTAMP            ACTION       CLIENT         BY        REASON
#   2025-12-06T14:23:00  disconnect   broken-client  system    buffer_overflow
#   2025-12-06T14:25:00  block        broken-client  operator  investigation
#   2025-12-06T15:00:00  unblock      broken-client  operator  fixed
```

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

### Client Administration CLI

Comprehensive client lifecycle management:

```bash
# ─────────────────────────────────────────────────────────────
# Client lifecycle
# ─────────────────────────────────────────────────────────────

# Create client
ssmd client create trading-bot-1 \
  --type non-display \
  --env prod \
  --owner "trading-team@example.com" \
  --description "Algo trading bot for BTC strategies"

# List clients
ssmd client list --env prod
ssmd client list --env prod --type display
ssmd client list --env prod --owner "*@trading.com"

# Show client details
ssmd client show trading-bot-1 --env prod
# Output:
#   Client: trading-bot-1
#   Type: non-display
#   Owner: trading-team@example.com
#   Created: 2025-12-01
#   API Key: sk_...abc (expires: 2026-03-01)
#   Status: active
#   Entitlements:
#     - kraken: BTC*, ETH* (trade, quote)
#     - coinbase: * (trade)
#   Current sessions: 2
#   Messages today: 1,234,567

# Update client
ssmd client update trading-bot-1 --env prod --owner "new-owner@example.com"
ssmd client update trading-bot-1 --env prod --type display

# Disable/enable client (soft delete)
ssmd client disable trading-bot-1 --env prod --reason "contract ended"
ssmd client enable trading-bot-1 --env prod

# Delete client (hard delete, requires confirmation)
ssmd client delete trading-bot-1 --env prod --confirm

# ─────────────────────────────────────────────────────────────
# API key management
# ─────────────────────────────────────────────────────────────

# Generate initial API key
ssmd client api-key create trading-bot-1 --env prod --expires 90d
# Output:
#   API Key: sk_live_abc123...xyz
#   Expires: 2026-03-06
#
#   ⚠️  Save this key now - it won't be shown again!

# List API keys (shows masked keys)
ssmd client api-key list trading-bot-1 --env prod
# Output:
#   ID       Created     Expires     Status    Last Used
#   key_001  2025-12-01  2026-03-01  active    2025-12-06 14:23
#   key_002  2025-11-15  2026-02-15  revoked   2025-11-20 09:00

# Rotate API key (old key valid during grace period)
ssmd client api-key rotate trading-bot-1 --env prod --grace-period 24h
# Output:
#   New API Key: sk_live_def456...uvw
#   Old key valid until: 2025-12-07 14:30
#
#   ⚠️  Update your application within 24 hours!

# Revoke API key immediately
ssmd client api-key revoke trading-bot-1 --env prod --key-id key_001

# Revoke all keys (emergency)
ssmd client api-key revoke-all trading-bot-1 --env prod --reason "security incident"

# ─────────────────────────────────────────────────────────────
# Entitlement management
# ─────────────────────────────────────────────────────────────

# Grant entitlement
ssmd client entitle trading-bot-1 --env prod \
  --feed kraken \
  --symbols "BTC*,ETH*" \
  --data-types trade,quote \
  --expires 2026-01-01

# Grant full feed access
ssmd client entitle trading-bot-1 --env prod \
  --feed kraken \
  --symbols "*" \
  --data-types trade,quote,book

# List entitlements
ssmd client entitlements trading-bot-1 --env prod
# Output:
#   Feed      Symbols    Data Types       Expires
#   kraken    BTC*,ETH*  trade,quote      2026-01-01
#   kraken    SOL*       trade            never
#   coinbase  *          trade,quote,book 2026-06-01

# Revoke specific entitlement
ssmd client revoke trading-bot-1 --env prod --feed kraken --symbols "ETH*"

# Revoke all entitlements for a feed
ssmd client revoke trading-bot-1 --env prod --feed kraken

# Revoke all entitlements (client can still auth, just can't subscribe)
ssmd client revoke-all trading-bot-1 --env prod

# ─────────────────────────────────────────────────────────────
# Session & connection management
# ─────────────────────────────────────────────────────────────

# View active sessions
ssmd client sessions trading-bot-1 --env prod
# Output:
#   Session     Connected        IP            Device       Subscriptions
#   sess_001    2025-12-06 14:00 10.0.1.50     server-1     BTCUSD, ETHUSD
#   sess_002    2025-12-06 14:15 10.0.1.51     server-2     BTCUSD

# Disconnect specific session
ssmd client disconnect trading-bot-1 --env prod --session sess_001

# Disconnect all sessions
ssmd client disconnect-all trading-bot-1 --env prod --reason "maintenance"

# Set concurrent session limit
ssmd client limit trading-bot-1 --env prod --max-sessions 5

# ─────────────────────────────────────────────────────────────
# Rate limiting & quotas
# ─────────────────────────────────────────────────────────────

# Set message rate limit
ssmd client rate-limit trading-bot-1 --env prod --max-rate 50000/s

# Set daily message quota
ssmd client quota trading-bot-1 --env prod --daily-limit 100000000

# View current usage
ssmd client usage trading-bot-1 --env prod
ssmd client usage trading-bot-1 --env prod --period month

# ─────────────────────────────────────────────────────────────
# Bulk operations
# ─────────────────────────────────────────────────────────────

# Import clients from file
ssmd client import --env prod --file clients.yaml --dry-run
ssmd client import --env prod --file clients.yaml

# Export clients (for backup or migration)
ssmd client export --env prod > clients-backup.yaml

# Bulk entitle (e.g., new feed goes live)
ssmd client entitle-bulk --env prod \
  --filter "type=non-display" \
  --feed newexchange \
  --symbols "*" \
  --dry-run
```

**clients.yaml format:**
```yaml
clients:
  - id: trading-bot-1
    type: non-display
    owner: trading@example.com
    entitlements:
      - feed: kraken
        symbols: ["BTC*", "ETH*"]
        data_types: [trade, quote]
        expires: 2026-01-01

  - id: research-dashboard
    type: display
    owner: research@example.com
    max_sessions: 10
    entitlements:
      - feed: kraken
        symbols: ["*"]
        data_types: [trade, quote]
```

**Audit endpoints:**
```
GET /api/v1/audit/usage?month=2025-12&feed=kraken
GET /api/v1/audit/peak-users?date=2025-12-06
GET /api/v1/audit/display-vs-nondisplay?month=2025-12
```

**Storage:** PostgreSQL

## TUI Admin

Terminal interface for operating ssmd:

```
┌─ ssmd ──────────────────────────────────────────────────┐
│                                                         │
│  Environment: prod    Status: ● Healthy                 │
│                                                         │
│  ┌─ Shards ───────────────────────────────────────────┐ │
│  │ ● tier1   3 symbols   45k msg/s   lag: 2ms         │ │
│  │ ● tier2   47 symbols  12k msg/s   lag: 5ms         │ │
│  └────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ Feeds ────────────────────────────────────────────┐ │
│  │ ● kraken    50 symbols   57k msg/s   lag: 3ms      │ │
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
│  [s]hards  [f]eeds  [c]lients  [t]orage  [l]ogs  [q]uit│
└─────────────────────────────────────────────────────────┘
```

**Capabilities:**
- View shard status, symbol counts, and per-shard throughput
- View live feed status and message rates
- Monitor connected clients and usage
- Check storage consumption
- Tail logs with filtering
- Manage entitlements (add/revoke clients)
- Trigger manual archival or replay

**Built with:** Go + bubbletea/lipgloss

## Audit & Compliance

Comprehensive audit trail for operational, security, and regulatory requirements.

### Audit Event Schema

All audit events share a consistent structure:

```yaml
audit_event:
  id: "evt_abc123"
  timestamp: "2025-12-06T14:23:00.123456Z"
  actor:
    type: user | system | client | operator
    id: "operator@example.com"
    ip: "10.0.1.50"
    location: "us-east-1a"
  action: "client.disconnect"
  target:
    type: client | instrument | environment | entitlement
    id: "broken-client"
  outcome: success | failure
  reason: "buffer_overflow"
  metadata:
    buffer_usage: "100%"
    lag_seconds: 120
```

### What Gets Audited

| Category | Events |
|----------|--------|
| **Client Management** | Connect, disconnect, block, unblock, rate-limit change |
| **Entitlements** | Grant, revoke, modify, expire |
| **Secmaster** | Instrument add/modify, mapping changes, FIGI sync |
| **Environment** | Create, apply, delete, reshard |
| **Data Operations** | Reprocess trigger, archive, export |
| **Configuration** | Calendar changes, alert rule changes |
| **Security** | Auth failures, permission denied, API key rotation |
| **System** | Connector restart, shard failover, clock sync issues |

### Storage

**Short-term (hot):** PostgreSQL for fast queries (30 days)
**Long-term (cold):** Archived to Garage in JSONL format (forever)

```
/audit/2025/12/06/events.jsonl.zst
```

### CLI Commands

```bash
# Query audit log
ssmd audit list --days 7
ssmd audit list --actor operator@example.com
ssmd audit list --action "client.*"
ssmd audit list --target-type client --target-id broken-client

# Export for compliance
ssmd audit export --start 2025-01-01 --end 2025-12-31 --format csv > audit-2025.csv

# Specific audit views
ssmd audit client-actions --env prod --days 30
ssmd audit entitlement-changes --env prod --days 90
ssmd audit security-events --days 7
ssmd audit config-changes --days 30
```

### API Endpoints

```
GET /api/v1/audit/events?days=7&actor=...&action=...
GET /api/v1/audit/events/{event_id}
GET /api/v1/audit/summary?days=30  # Counts by category
```

### Compliance Reports

Pre-built reports for common requirements:

```bash
# Monthly entitlement audit
ssmd report entitlements --month 2025-12

# Data access report (who accessed what)
ssmd report data-access --client trading-bot-1 --month 2025-12

# System changes report
ssmd report changes --month 2025-12

# Exchange audit package (for vendor audits)
ssmd report exchange-audit --feed kraken --month 2025-12 --output kraken-audit.zip
```

### Retention Policy

| Data Type | Retention | Rationale |
|-----------|-----------|-----------|
| Security events | 7 years | Regulatory requirement |
| Entitlement changes | 7 years | Audit trail |
| Client actions | 2 years | Operational |
| Config changes | 2 years | Operational |
| System events | 90 days | Debugging |

## Security

Defense in depth across all layers.

### Authentication

**API Authentication:**
```yaml
auth:
  methods:
    - api_key        # Primary for programmatic access
    - mtls           # Mutual TLS for high-security clients
    - oidc           # For human operators (SSO)
```

**API Key Management:**
```bash
# Generate API key for client
ssmd client api-key create trading-bot-1 --env prod --expires 90d
# Output: ssmd_key_abc123...

# Rotate API key
ssmd client api-key rotate trading-bot-1 --env prod --grace-period 24h

# Revoke immediately
ssmd client api-key revoke trading-bot-1 --env prod

# List keys (shows last used, expiry)
ssmd client api-key list --env prod
```

### Authorization

**Role-Based Access Control (RBAC):**

| Role | Capabilities |
|------|--------------|
| `viewer` | Read data, view status |
| `operator` | + Disconnect clients, view audit |
| `admin` | + Manage entitlements, modify config |
| `superadmin` | + Delete environments, access all tenants |

**Per-environment permissions:**
```yaml
# rbac/roles.yaml
users:
  operator@example.com:
    role: operator
    environments: [prod, staging]

  admin@example.com:
    role: admin
    environments: [prod, staging, dev]
```

### Network Security (Istio/Envoy)

Istio service mesh provides mTLS everywhere, traffic management, and observability.

```
┌─────────────────────────────────────────────────────────┐
│                    Internet                              │
└─────────────────────┬───────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────┐
│         Istio Ingress Gateway (Envoy)                   │
│         - TLS termination                               │
│         - Rate limiting                                 │
│         - JWT validation                                │
│         - Request routing                               │
└─────────────────────┬───────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────┐
│                    Gateway                               │
│         (with Envoy sidecar)                            │
│         - API key → entitlement lookup                  │
│         - Per-client buffering                          │
│         - Protocol handling                             │
└─────────────────────┬───────────────────────────────────┘
                      │ mTLS (automatic)
                      ▼
┌─────────────────────────────────────────────────────────┐
│              Service Mesh (all pods have sidecars)      │
│         ┌──────────┐  ┌──────────┐  ┌──────────┐       │
│         │Connector │  │ Archiver │  │   NATS   │       │
│         │ +sidecar │  │ +sidecar │  │ +sidecar │       │
│         └──────────┘  └──────────┘  └──────────┘       │
│                                                         │
│         ┌──────────┐  ┌──────────┐                     │
│         │  Garage  │  │ Temporal │                     │
│         │ +sidecar │  │ +sidecar │                     │
│         └──────────┘  └──────────┘                     │
└─────────────────────────────────────────────────────────┘
```

**What Istio provides:**

| Feature | How It Helps |
|---------|--------------|
| mTLS everywhere | Zero-trust networking, no plaintext internal traffic |
| Traffic policies | Fine-grained allow/deny between services |
| Rate limiting | Per-client, per-route limits at mesh level |
| JWT validation | Offload auth to sidecar |
| Observability | Automatic tracing, metrics for all service calls |
| Circuit breaking | Prevent cascade failures |
| Retries/timeouts | Configurable per-route |

**Istio configuration:**

```yaml
# PeerAuthentication: require mTLS everywhere
apiVersion: security.istio.io/v1beta1
kind: PeerAuthentication
metadata:
  name: default
  namespace: ssmd
spec:
  mtls:
    mode: STRICT

---
# AuthorizationPolicy: restrict traffic between services
apiVersion: security.istio.io/v1beta1
kind: AuthorizationPolicy
metadata:
  name: gateway-to-nats
  namespace: ssmd
spec:
  selector:
    matchLabels:
      app: nats
  rules:
    - from:
        - source:
            principals: ["cluster.local/ns/ssmd/sa/gateway"]
      to:
        - operation:
            ports: ["4222"]

---
# AuthorizationPolicy: connectors can only reach NATS and exchanges
apiVersion: security.istio.io/v1beta1
kind: AuthorizationPolicy
metadata:
  name: connector-egress
  namespace: ssmd
spec:
  selector:
    matchLabels:
      app: connector
  rules:
    - to:
        - operation:
            hosts: ["nats.ssmd.svc.cluster.local"]
    - to:
        - operation:
            hosts: ["*.kraken.com", "*.coinbase.com"]

---
# RequestAuthentication: JWT at ingress
apiVersion: security.istio.io/v1beta1
kind: RequestAuthentication
metadata:
  name: jwt-auth
  namespace: ssmd
spec:
  selector:
    matchLabels:
      app: gateway
  jwtRules:
    - issuer: "https://auth.ssmd.example.com"
      jwksUri: "https://auth.ssmd.example.com/.well-known/jwks.json"
```

**Rate limiting with Envoy:**

```yaml
apiVersion: networking.istio.io/v1alpha3
kind: EnvoyFilter
metadata:
  name: rate-limit
  namespace: ssmd
spec:
  workloadSelector:
    labels:
      app: gateway
  configPatches:
    - applyTo: HTTP_FILTER
      match:
        context: SIDECAR_INBOUND
      patch:
        operation: INSERT_BEFORE
        value:
          name: envoy.filters.http.local_ratelimit
          typed_config:
            "@type": type.googleapis.com/envoy.extensions.filters.http.local_ratelimit.v3.LocalRateLimit
            stat_prefix: http_local_rate_limiter
            token_bucket:
              max_tokens: 10000
              tokens_per_fill: 1000
              fill_interval: 1s
```

**Observability (automatic with Istio):**

```yaml
# Kiali: service mesh visualization
# Jaeger: distributed tracing
# Prometheus: metrics (auto-scraped from sidecars)

# Access via:
# - istioctl dashboard kiali
# - istioctl dashboard jaeger
```

**Network policies (in addition to Istio):**

```yaml
# Kubernetes NetworkPolicy as defense in depth
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: deny-external-to-internal
  namespace: ssmd
spec:
  podSelector:
    matchLabels:
      tier: internal  # connectors, archivers, nats, garage
  policyTypes:
    - Ingress
  ingress:
    - from:
        - podSelector: {}  # Only from within namespace
```

**Traffic flow rules:**

| From | To | Allowed |
|------|-----|---------|
| Ingress Gateway | Gateway | ✓ (external traffic entry point) |
| Gateway | NATS | ✓ |
| Gateway | Garage | ✓ (historical queries) |
| Connectors | NATS | ✓ |
| Connectors | Exchanges (egress) | ✓ |
| Archivers | NATS | ✓ |
| Archivers | Garage | ✓ |
| NATS | External | ✗ |
| Garage | External | ✗ |
| Any internal | Any internal | mTLS required |

### Secrets Management

```yaml
secrets:
  storage: kubernetes-secrets | vault | sops

  # What needs secrets
  exchange_credentials:
    kraken_api_key: vault:secret/ssmd/kraken#api_key
    kraken_api_secret: vault:secret/ssmd/kraken#api_secret

  database:
    postgres_password: vault:secret/ssmd/postgres#password

  api_keys:
    signing_key: vault:secret/ssmd/api#signing_key
```

**Never in Git:**
- API credentials
- Encryption keys
- Client API keys

### Encryption

| Data | At Rest | In Transit |
|------|---------|------------|
| Raw market data | Garage encryption (optional) | TLS 1.3 |
| Normalized data | Garage encryption (optional) | TLS 1.3 |
| Audit logs | PostgreSQL encryption | TLS 1.3 |
| Secrets | Vault/SOPS | mTLS |
| NATS messages | Stream encryption (optional) | TLS 1.3 |

### Security Monitoring

**Metrics:**
```
ssmd_auth_attempts_total{outcome="success|failure", method="api_key|mtls"}
ssmd_auth_failures_total{reason="invalid_key|expired|revoked"}
ssmd_permission_denied_total{action="...", actor="..."}
ssmd_rate_limit_exceeded_total{client="..."}
```

**Alerts:**
```yaml
alerts:
  - name: auth-failure-spike
    condition: rate(ssmd_auth_failures_total[5m]) > 10
    severity: warning

  - name: brute-force-attempt
    condition: rate(ssmd_auth_failures_total{reason="invalid_key"}[1m]) > 20
    severity: critical
    action: auto-block-ip

  - name: permission-denied-spike
    condition: rate(ssmd_permission_denied_total[5m]) > 5
    severity: warning
```

### Security CLI

```bash
# View security events
ssmd security events --days 7

# Check for anomalies
ssmd security scan

# Rotate all secrets
ssmd security rotate-secrets --component gateway

# IP blocklist management
ssmd security block-ip 1.2.3.4 --reason "brute force" --duration 24h
ssmd security unblock-ip 1.2.3.4
ssmd security blocklist
```

### Incident Response

```bash
# Emergency: revoke all access for a client
ssmd client emergency-revoke trading-bot-1 --env prod --reason "compromised"
# - Revokes API keys
# - Disconnects active sessions
# - Blocks reconnection
# - Creates high-priority audit event
# - Notifies operators

# Emergency: read-only mode (stop all writes)
ssmd env read-only prod --enable --reason "security incident"
```

### Security Checklist

| Item | Status |
|------|--------|
| All external traffic over TLS 1.3 | Required |
| Internal traffic over mTLS | Recommended |
| API keys rotated every 90 days | Policy |
| Secrets never in Git | Required |
| Audit logs immutable | Required |
| Network policies enforced | Required |
| Security events monitored | Required |
| Incident response documented | Required |

## Deployment & GitOps

### Repository Structure

```
ssmd/
├── cmd/                             # Go tooling
│   ├── ssmd-cli/
│   ├── ssmd-tui/
│   ├── ssmd-worker/                 # Temporal worker
│   └── ssmd-mcp/                    # MCP server for AI agents
├── pkg/                             # Go shared packages
│   ├── config/
│   ├── client/
│   └── workflows/                   # Temporal workflow definitions
├── src/                             # Zig data path
│   ├── connector/                   # Handles live, replay, reprocess modes
│   ├── archiver/
│   └── gateway/
├── proto/                           # Cap'n Proto schemas
├── environments/                    # Environment definitions
│   ├── dev.yaml
│   ├── staging.yaml
│   └── prod.yaml
├── secmaster/                       # Security master (FIGI-based)
│   ├── instruments.yaml             # Instrument definitions
│   └── mappings/                    # Per-feed symbol mappings
│       ├── kraken.yaml
│       ├── coinbase.yaml
│       └── polymarket.yaml
├── calendars/                       # Market calendars
│   ├── crypto.yaml
│   ├── us-equity.yaml
│   └── kraken.yaml
├── transforms/                      # Customer Lua transforms
│   └── customer-abc.lua
├── qa/                              # Data quality check definitions
│   └── kraken-checks.yaml
├── alerts/                          # Alert rules + Linear integration
│   └── linear-integration.yaml
├── charts/
│   └── ssmd/
│       ├── Chart.yaml
│       ├── values.yaml              # Defaults
│       ├── values-homelab.yaml      # Homelab overrides
│       └── templates/
│           ├── connector.yaml       # Generates one Deployment per shard
│           ├── archiver.yaml        # Generates one Deployment per shard
│           ├── gateway.yaml
│           ├── nats.yaml
│           ├── nats-mirroring.yaml  # Subject mirroring config
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
# Connector (per shard)
ssmd_connector_messages_received_total{shard="tier1", feed="kraken", symbol="BTCUSD"}
ssmd_connector_websocket_reconnects_total{shard="tier1", feed="kraken"}
ssmd_connector_lag_seconds{shard="tier1", feed="kraken"}

# Shard-level aggregates
ssmd_shard_symbols_count{shard="tier1", env="prod"}
ssmd_shard_messages_per_second{shard="tier1", env="prod"}

# Gateway (unified, shard-agnostic)
ssmd_gateway_clients_connected{type="display"}
ssmd_gateway_messages_delivered_total{client="agent-1"}

# Archiver (per shard)
ssmd_archiver_bytes_written_total{shard="tier1", bucket="normalized"}
ssmd_archiver_lag_seconds{shard="tier1"}
```

### Logs

Structured JSON to stdout, collected with Loki.

### Healthchecks

```
GET /health         # Alive check
GET /health/ready   # Dependencies connected
```

### Alerting Rules

- Connector websocket disconnected > 30s (per shard)
- Archiver lag > 5 minutes (per shard)
- Shard lag > 5 seconds
- Shard throughput dropped > 50% vs baseline
- Shard imbalance (one shard handling >80% of total volume)
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

## Automated Quality Assurance

No QA team. All testing is automated and reproducible.

### Version Comparison Environments

Spin up ephemeral environments to compare versions:

```bash
# Compare current vs new normalizer
ssmd env compare \
  --baseline prod \
  --candidate pr-123 \
  --feed kraken \
  --date 2025-12-05 \
  --output diff-report.json
```

**How it works:**
1. Spin up ephemeral k8s namespace with candidate version
2. Replay raw data from specified date through both versions
3. Compare normalized output (schema, values, counts)
4. Generate diff report
5. Tear down ephemeral environment

**CI/CD integration:**
```yaml
# .github/workflows/compare.yml
on: pull_request
jobs:
  compare:
    runs-on: self-hosted  # Needs cluster access
    steps:
      - uses: actions/checkout@v4
      - name: Compare with production
        run: |
          ssmd env compare \
            --baseline prod \
            --candidate ${{ github.sha }} \
            --feed kraken \
            --date $(date -d yesterday +%Y-%m-%d) \
            --output diff-report.json
      - name: Post diff summary
        run: ssmd report post-github --file diff-report.json
```

**Diff report includes:**
- Record count differences
- Schema changes
- Value distribution changes
- Timing differences
- Sample mismatches for inspection

### Data Quality Checks

Automated checks run via Temporal on every archive:

```yaml
# qa/kraken-checks.yaml
feed: kraken
checks:
  - name: no-gaps
    type: sequence
    max_gap_seconds: 60

  - name: price-sanity
    type: range
    field: price
    min: 0
    max: 1000000

  - name: symbol-coverage
    type: presence
    symbols: [BTCUSD, ETHUSD]
    min_records_per_hour: 100

  - name: timestamp-order
    type: monotonic
    field: timestamp
```

**Results:**
- Stored in Garage (`/qa/kraken/2025/12/05/results.json`)
- Failures create alerts
- Historical trends visible in Grafana

## Linear Integration

No large support team. Customer issues flow directly into development.

### Issue Flow

```
Customer reports issue
        │
        ▼
┌─────────────────┐
│    Linear       │◀─── Observability alerts also create issues
│  (issue inbox)  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Triage        │ Weekly review, prioritize
│   (label/rank)  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Roadmap       │ Scheduled for sprint
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Development   │ PR links to Linear issue
└─────────────────┘
```

### Auto-Created Issues

Observability → Linear integration:

```yaml
# alerts/linear-integration.yaml
alerts:
  - name: connector-down
    condition: ssmd_connector_up == 0
    for: 5m
    linear:
      team: platform
      priority: urgent
      template: |
        **Connector Down**
        Feed: {{ $labels.feed }}
        Duration: {{ $value }}m

  - name: data-quality-failure
    condition: ssmd_qa_check_failed > 0
    linear:
      team: data-quality
      priority: high
      template: |
        **QA Check Failed**
        Feed: {{ $labels.feed }}
        Check: {{ $labels.check }}
        Date: {{ $labels.date }}
```

### CLI Integration

```bash
ssmd issue list                          # Show open issues from Linear
ssmd issue link LIN-123 --pr 456         # Link Linear issue to PR
ssmd issue close LIN-123 --release v1.2  # Close with release note
```

## Job Scheduling (Temporal)

Temporal handles all scheduled and long-running jobs with market calendar awareness.

### Why Temporal

- **Durable execution** - Jobs survive restarts, automatic retries
- **Custom calendars** - Schedule on trading days only, skip holidays
- **Visibility** - Web UI shows job history, failures, retries
- **Workflow orchestration** - Multi-step jobs with dependencies

### Market Calendars

```yaml
# calendars/us-equity.yaml
name: us-equity
timezone: America/New_York
trading_days:
  - weekdays: [mon, tue, wed, thu, fri]
trading_hours:
  open: "09:30"
  close: "16:00"
holidays:
  - 2025-01-01  # New Year's Day
  - 2025-01-20  # MLK Day
  - 2025-02-17  # Presidents Day
  - 2025-04-18  # Good Friday
  - 2025-05-26  # Memorial Day
  - 2025-06-19  # Juneteenth
  - 2025-07-04  # Independence Day
  - 2025-09-01  # Labor Day
  - 2025-11-27  # Thanksgiving
  - 2025-12-25  # Christmas

# calendars/crypto.yaml
name: crypto
timezone: UTC
trading_days:
  - weekdays: [mon, tue, wed, thu, fri, sat, sun]  # 24/7
trading_hours:
  open: "00:00"
  close: "23:59"
holidays: []  # Never closed
```

### Job Types

**Scheduled jobs:**

| Job | Schedule | Calendar | Description |
|-----|----------|----------|-------------|
| daily-archive | 06:00 | per-exchange | Archive previous day's JetStream to Garage |
| daily-qa | 07:00 | per-exchange | Run data quality checks on yesterday's data |
| weekly-cleanup | Sun 02:00 | none | Prune old normalized versions |
| monthly-audit | 1st 08:00 | none | Generate usage/entitlement reports |

**On-demand jobs:**

| Job | Trigger | Description |
|-----|---------|-------------|
| reprocess | CLI / API | Rebuild normalized data for date range |
| backfill | CLI / API | Fill gaps in raw data (if source supports) |
| export | CLI / API | Export data to external format (Parquet, etc.) |

### Workflow Examples

**Daily archive workflow:**
```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Wait for   │────▶│  Archive    │────▶│  Verify     │
│  market     │     │  JetStream  │     │  integrity  │
│  close      │     │  → Garage   │     │             │
└─────────────┘     └─────────────┘     └──────┬──────┘
                                               │
                                               ▼
                                        ┌─────────────┐
                                        │  Notify on  │
                                        │  failure    │
                                        └─────────────┘
```

**Reprocess workflow:**
```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Validate   │────▶│  Process    │────▶│  Compare    │
│  date range │     │  each day   │     │  old vs new │
│             │     │  (parallel) │     │             │
└─────────────┘     └─────────────┘     └──────┬──────┘
                                               │
                                               ▼
                                        ┌─────────────┐
                                        │  Promote    │
                                        │  version    │
                                        └─────────────┘
```

### CLI Integration

```bash
# Calendar management
ssmd calendar list
ssmd calendar add us-equity --from-file calendars/us-equity.yaml
ssmd calendar holidays us-equity --year 2025

# Job management
ssmd job list                                   # List running/recent jobs
ssmd job list --status failed --days 7          # Filter by status
ssmd job status <job-id>                        # Detailed job info
ssmd job cancel <job-id>                        # Cancel running job
ssmd job retry <job-id>                         # Retry failed job
ssmd job logs <job-id>                          # Stream job logs

# Schedule management
ssmd schedule list                              # List all schedules
ssmd schedule show daily-archive                # Schedule details + history
ssmd schedule pause daily-archive               # Pause schedule
ssmd schedule resume daily-archive              # Resume schedule
ssmd schedule trigger daily-archive             # Run now (out of schedule)
ssmd schedule create my-job --cron "0 6 * * *" --workflow daily-archive

# Reprocessing (connector in replay mode with different version)
ssmd reprocess list                             # List reprocess jobs
ssmd reprocess status                           # Show what needs reprocessing

# Reprocess specific date range (runs connector in reprocess mode)
ssmd reprocess run --feed kraken --start 2025-12-01 --end 2025-12-05

# Reprocess with specific connector version (e.g., after bugfix)
ssmd reprocess run --feed kraken --date 2025-12-06 --connector-version v1.2.4

# Reprocess to new schema version
ssmd reprocess run --feed kraken --date 2025-12-06 --schema v2

# Dry run - show what would be reprocessed
ssmd reprocess run --feed kraken --date 2025-12-06 --dry-run
# Output:
#   Would reprocess 1,440 files (24 hours × 60 minutes)
#   Source: /raw/kraken/2025/12/06/
#   Target: /normalized/v2/kraken/...
#   Connector: v1.2.4 (abc1234)
#   Estimated time: ~15 minutes

# Compare old vs new processing
ssmd reprocess diff --feed kraken --date 2025-12-06 --old v1 --new v2
```

**Reprocessing = Connector in replay mode:**

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   CLI       │───▶│  Temporal   │───▶│  Connector  │───▶│   Garage    │
│  reprocess  │    │  Workflow   │    │  (reprocess │    │ /normalized │
│             │    │             │    │    mode)    │    │    /v2/     │
└─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘
                                             │
                                             ▼
                                      ┌─────────────┐
                                      │   Garage    │
                                      │   /raw/     │ (input)
                                      └─────────────┘
```

**Key principles:**
- Same connector binary, different mode (`--mode reprocess`)
- Raw data is immutable source of truth
- Reprocessing creates new version directories (v1, v2, ...)
- Connector version embedded in output for traceability
- No separate "reprocessor" component - just connector + Temporal

### Temporal Deployment

```yaml
# In Helm chart
temporal:
  enabled: true
  server:
    replicas: 1  # Homelab scale
  ui:
    enabled: true
  persistence:
    default:
      driver: postgres
```

**Temporal UI** available at `http://temporal.ssmd.local` for:
- Viewing workflow history
- Inspecting failed jobs
- Manual retry/cancel
- Schedule management

## Implementation Phases

### Phase 0: CLI & Environment Definitions (Build First)

Metadata support must come first. Remove chance of operator error.

- Build ssmd-cli skeleton in Go (cobra)
- Define environment YAML schema (including symbols, shards, subjects)
- Implement `ssmd env create`, `validate`, `apply`
- Implement `ssmd feed add/remove/list`
- Implement `ssmd symbol` commands (with attribute support)
- Implement `ssmd shard list/status/symbols`
- Implement shard validation (all symbols covered, no overlaps)
- **Implement secmaster with FIGI support:**
  - `ssmd secmaster add/show/history` commands
  - `ssmd secmaster map` for feed mappings
  - `ssmd secmaster lookup` (by symbol, FIGI, or external)
  - Temporal queries (`--as-of` flag)
  - OpenFIGI API integration (`ssmd secmaster figi-lookup`)
- Generate Helm values from environment spec (one deployment per shard)
- Validate: can create dev environment, generate valid Helm values

### Phase 1: Foundation

- Set up k3s cluster on homelab
- Deploy NATS + JetStream via Helm
- Configure NATS subject mirroring (internal.* → md.*)
- Configure NATS KV stores (config, secmaster)
- Deploy Garage via Helm
- Deploy Temporal via Helm
- Deploy ArgoCD, connect to repo
- Use ssmd-cli to create and apply dev environment
- Load secmaster into NATS KV
- Define crypto calendar (24/7)
- Validate: publish/subscribe to NATS (both internal and client subjects), write to Garage, Temporal UI accessible, secmaster lookups work

### Phase 2: Core Ingestion

- Decide: port olalla/libechidna to Zig or use existing C++
- Define Cap'n Proto schemas
- Implement/adapt websocket client + raw capture
- Connector reads `SHARD_ID` from environment, resolves to symbol list
- Connector uses secmaster to map internal symbols → feed-specific external symbols
- Connector watches secmaster for dynamic instrument additions
- Connector publishes to internal NATS subjects (`internal.{shard}.*`)
- Deploy multiple connector shards (tier1, tier2)
- Validate: see live data via `nats sub` on both internal and client subjects
- Validate: add dynamic instrument, connector picks it up without restart

### Phase 3: Archival & Scheduling

- Build archiver (JetStream → Garage), shard-aware
- Archiver consumes from `internal.{shard}.*` subjects
- Deploy multiple archiver shards (mirroring connectors)
- Add connector reprocess mode (Garage/raw → Garage/normalized)
- Build ssmd-worker (Temporal workflows)
- Implement daily-archive workflow
- Implement reprocess workflow (spawns connector in reprocess mode)
- Implement `ssmd calendar`, `ssmd job`, `ssmd reprocess` CLI commands
- Validate: data in Garage buckets from all shards, scheduled jobs run, reprocessing works

### Phase 4: Agent Access

- Build gateway (WebSocket + REST)
- Gateway subscribes to client-facing subjects (`md.*`), unaware of shards
- Add entitlements (PostgreSQL, API key validation)
- Implement `ssmd client add/entitle/revoke`
- Build MCP server (ssmd-mcp) with tools for market data, instruments, status
- Implement `ssmd mcp` CLI commands
- Validate: Python notebook can subscribe, AI agent can use MCP tools

### Phase 5: Operations

- Build TUI (with shards panel)
- Add Prometheus metrics (with shard labels)
- Set up Grafana dashboards (per-shard views)
- Add alerting rules (including shard-specific alerts)

### Phase 6: Polish

- Audit reporting endpoints
- Documentation
- Implement `ssmd env promote` for staging→prod workflow
- Test blue-green resharding workflow
- Add second exchange to validate normalization layer

## Resource Estimates (Homelab)

### Compute

| Component | CPU | Memory | Notes |
|-----------|-----|--------|-------|
| ssmd-connector (per shard) | 0.1-0.5 core | 64-256 MB | Depends on shard volume |
| ssmd-archiver (per shard) | 0.1-0.2 core | 128 MB | Batch writes, mostly idle |
| ssmd-gateway | 0.2 core | 128 MB | Scales with connected clients |
| ssmd-mcp | 0.1 core | 64 MB | MCP server for AI agents |
| ssmd-worker | 0.2 core | 256 MB | Temporal workflows + Lua transforms |
| NATS + JetStream | 0.5 core | 512 MB | Depends on message volume |
| Temporal | 0.5 core | 512 MB | Workflow orchestration |
| Garage (3-node min) | 0.3 core × 3 | 256 MB × 3 | Distributed, needs 3 nodes |
| ArgoCD | 0.3 core | 512 MB | Can share with other workloads |
| Prometheus | 0.2 core | 512 MB | Depends on retention |
| Grafana | 0.1 core | 256 MB | Mostly idle |
| Loki | 0.2 core | 256 MB | Depends on log volume |

**Sharding impact:** With 2 shards, add ~0.4 core and 400 MB for connector + archiver pairs. Resource allocation per shard is configurable in the environment spec.

**Minimum total:** ~3 cores, 4 GB RAM (tight, single-node k3s, 1-2 shards)

**Recommended:** 4-6 cores, 8-16 GB RAM (comfortable headroom, 2-4 shards)

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
| Tooling Language | Go |
| Data Path Language | Zig or C++ (TBD) |
| Serialization | Cap'n Proto |
| Messaging | NATS + JetStream (initial, abstracted) |
| Object Storage | Garage |
| Database | PostgreSQL |
| Job Scheduling | Temporal |
| Container Orchestration | Kubernetes (k3s) |
| Service Mesh | Istio/Envoy |
| Package Management | Helm |
| GitOps | ArgoCD |
| Monitoring | Prometheus + Grafana + Loki |
| Issue Tracking | Linear |
| Customer Transforms | Lua (gopher-lua) |
| AI Agent Protocol | MCP (Model Context Protocol) |
| CLI Framework | Go + cobra |
| TUI Framework | Go + bubbletea |
| Initial Exchange | Kraken |

## Simplicity Metrics

"Simple" must be measurable. These are the operational simplicity targets for ssmd:

### Operational Targets

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Deploy from zero | ≤ 5 commands | Count commands in quickstart |
| Add new symbol | 1 config change, 0 restarts | Hot reload via NATS |
| Add new shard | 1 config change + apply | Edit environment, `ssmd env apply` |
| Reshard (blue-green) | 4 commands | create → edit → apply → delete old |
| Add new exchange | 1 new file + config | Connector per exchange |
| Recover from pod crash | Automatic, < 60s | k8s restart, verify with chaos testing |
| Recover from shard failure | Other shards unaffected | Chaos testing per shard |
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
| Reprocess 1 day of raw data | < 1 hour | `ssmd reprocess` runtime |
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

## Open Decisions

| Decision | Options | Notes |
|----------|---------|-------|
| Connector language | Zig (port) vs C++ (existing olalla/libechidna) | Existing C++ has battle-tested io_uring |
| Garage single-node | Run Garage with 1 node for homelab? | Officially needs 3 nodes, but 1 may work |
| Transport interface design | Thin wrapper vs full abstraction | Need to balance flexibility with complexity; Aeron/Chronicle have different semantics than NATS |

## Document History

| Date | Changes |
|------|---------|
| 2025-12-06 | Initial design covering architecture, data schema, connectors, archival, agent access, entitlements, TUI, deployment, observability, QA, Linear integration, Temporal scheduling, simplicity metrics |
| 2025-12-06 | Added sharding design: attribute-based symbol sharding, mirrored connector/collector sharding, NATS subject architecture, multi-tenancy model, validation rules, resharding operations |
| 2025-12-06 | Updated all sections for sharding consistency: architecture diagram, environment spec, NATS subjects, connector/archiver designs, TUI, metrics, alerts, implementation phases, resource estimates, simplicity metrics |
| 2025-12-06 | Added transport abstraction for future Aeron/Chronicle support |
| 2025-12-06 | Added Security Master with FIGI support, temporal queries (as-of date), dynamic instrument creation for prediction markets |
| 2025-12-06 | Added Consumer Backpressure & Defensive Design: pull consumers, per-client buffers, slow consumer detection, client management CLI, protection layers |
| 2025-12-06 | Added Capture Provenance: location tracking, clock quality metadata, monitoring |
| 2025-12-06 | Added Audit & Compliance: event schema, retention policies, compliance reports |
| 2025-12-06 | Added Security: authentication (API key, mTLS, OIDC), RBAC, network security, secrets management, incident response |
| 2025-12-06 | Updated Network Security to use Istio/Envoy service mesh: mTLS everywhere, AuthorizationPolicy, RequestAuthentication for JWT, EnvoyFilter for rate limiting |
| 2025-12-06 | Changed database from SQLite to PostgreSQL for all storage (entitlements, audit, secmaster) |
| 2025-12-06 | Added software lineage: connector version in capture metadata, processor version in archive file headers for reproducibility |
| 2025-12-06 | Expanded CLI for jobs/schedules: `ssmd job`, `ssmd schedule`, `ssmd reprocess` commands for managing Temporal workflows and rebuilding artifacts with specific versions |
| 2025-12-06 | Added Garage object versioning: S3-compatible versioning, lifecycle policies, `ssmd storage` CLI for version management |
| 2025-12-06 | Added Enrichment Pipeline: secmaster join, derived fields, quality flags, cross-feed enrichment with config and CLI |
| 2025-12-06 | Unified connector with input/output abstraction: live (websocket→NATS), replay (Garage→NATS), reprocess (Garage→Garage). No separate reprocessor component. |
| 2025-12-06 | Document cleanup: removed ssmd-reprocessor from components, updated directory structure, implementation phases, and metrics to reflect unified connector |
| 2025-12-06 | Added MCP Server (ssmd-mcp): AI agent native tool access via Model Context Protocol, tools for market data/instruments/status, resources for discovery |
| 2025-12-06 | Added Client Administration CLI: comprehensive `ssmd client` commands for lifecycle, API keys, entitlements, sessions, rate limits, bulk operations |
