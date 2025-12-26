# ssmd: Kalshi Design - Sharding & Scaling

The system shards data by metadata attributes and scales horizontally. Auto-scaling is supported where possible; resharding handles growth beyond auto-scale limits.

## Sharding Model

```
                              Symbol Attributes (Metadata)
                              ┌─────────────────────────┐
                              │ tier: 1, 2, 3          │
                              │ feed: kalshi, poly     │
                              │ category: crypto, fin  │
                              └───────────┬─────────────┘
                                          │
                    ┌─────────────────────┼─────────────────────┐
                    ▼                     ▼                     ▼
             ┌───────────┐         ┌───────────┐         ┌───────────┐
             │  Shard 1  │         │  Shard 2  │         │  Shard 3  │
             │  tier: 1  │         │  tier: 2  │         │  tier: 3  │
             └─────┬─────┘         └─────┬─────┘         └─────┬─────┘
                   │                     │                     │
         ┌─────────┴─────────┐   ┌───────┴───────┐   ┌─────────┴─────────┐
         ▼                   ▼   ▼               ▼   ▼                   ▼
    ┌─────────┐         ┌─────────┐         ┌─────────┐         ┌─────────┐
    │Connector│         │Archiver │         │Connector│         │Archiver │
    │ shard-1 │         │ shard-1 │         │ shard-2 │         │ shard-2 │
    └─────────┘         └─────────┘         └─────────┘         └─────────┘
```

## Shard Definition

Shards are defined in environment YAML using metadata selectors:

```yaml
# exchanges/environments/kalshi-prod.yaml
name: kalshi-prod
feed: kalshi
schema: trade:v1

# Symbol attributes (from secmaster or manual)
symbols:
  BTCUSD:
    tier: 1
    category: crypto
  ETHUSD:
    tier: 1
    category: crypto
  INXD-25-B4000:
    tier: 2
    category: financials

# Shard definitions with selectors
shards:
  tier1:
    selector:
      tier: 1
    replicas: 2              # Auto-scale up to 2 replicas
    resources:
      cpu: "500m"
      memory: "512Mi"

  tier2:
    selector:
      tier: 2
    replicas: 1
    resources:
      cpu: "250m"
      memory: "256Mi"

  # Category-based sharding
  crypto:
    selector:
      category: crypto
    replicas: 3              # High volume

  financials:
    selector:
      category: financials
    replicas: 1
```

## Shard Resolution

At startup, each component resolves its symbols from metadata:

```rust
pub struct ShardConfig {
    pub shard_id: String,
    pub selector: HashMap<String, String>,
    pub replicas: u32,
    pub resources: Resources,
}

impl ShardConfig {
    pub async fn resolve_symbols(&self, secmaster: &SecmasterClient) -> Result<Vec<Symbol>, Error> {
        // Query secmaster for symbols matching selector
        let symbols = secmaster
            .query_symbols()
            .filter(self.selector.clone())
            .execute()
            .await?;

        Ok(symbols)
    }
}

// Connector startup
pub async fn start_connector(env: &str, shard: &str) -> Result<(), Error> {
    let config = load_shard_config(env, shard).await?;
    let symbols = config.resolve_symbols(&secmaster).await?;

    info!(shard = %shard, symbols = symbols.len(), "Starting connector");

    // Subscribe only to assigned symbols
    for symbol in &symbols {
        exchange.subscribe(&symbol.external_id).await?;
    }

    // ...
}
```

## NATS Subject Sharding

Each shard publishes to its own subject namespace:

```
# Internal subjects (per shard)
internal.{shard}.{feed}.{type}.{symbol}
internal.tier1.kalshi.trade.BTCUSD
internal.tier2.kalshi.trade.INXD-25-B4000

# Client-facing subjects (merged via NATS mirroring)
md.{feed}.{type}.{symbol}
md.kalshi.trade.BTCUSD
```

Mirror configuration merges shard subjects:

```yaml
# NATS stream mirroring
streams:
  ssmd-internal-tier1:
    subjects: ["internal.tier1.>"]

  ssmd-internal-tier2:
    subjects: ["internal.tier2.>"]

  ssmd-client:
    sources:
      - name: ssmd-internal-tier1
        filter_subject: "internal.tier1.>"
        subject_transform:
          src: "internal.tier1.kalshi"
          dest: "md.kalshi"
      - name: ssmd-internal-tier2
        filter_subject: "internal.tier2.>"
        subject_transform:
          src: "internal.tier2.kalshi"
          dest: "md.kalshi"
```

## Auto-Scaling

Within a shard, replicas can auto-scale based on load:

```yaml
# Kubernetes HPA for connector
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: ssmd-connector-tier1
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: ssmd-connector-tier1
  minReplicas: 1
  maxReplicas: 3
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
    - type: Pods
      pods:
        metric:
          name: ssmd_connector_message_rate
        target:
          type: AverageValue
          averageValue: "10000"  # Messages per second per pod
```

**Auto-scale triggers:**
- CPU utilization > 70%
- Message rate per pod > threshold
- Memory pressure

**What auto-scaling handles:**
- Temporary load spikes
- Gradual growth within tier capacity

## Resharding

When auto-scaling isn't enough, resharding redistributes symbols:

```bash
# View current shard distribution
ssmd shard list kalshi-prod
# SHARD    SYMBOLS  REPLICAS  MSG/S    CPU%   MEM%
# tier1    5        2         45,000   68%    52%
# tier2    42       1         8,000    45%    38%

# Preview resharding
ssmd shard plan kalshi-prod --strategy split-tier1
# Proposed changes:
#   tier1 → tier1a (BTC*, ETH*)
#   tier1 → tier1b (SOL*, AVAX*)
# Estimated impact:
#   tier1a: ~25,000 msg/s
#   tier1b: ~20,000 msg/s

# Execute resharding (requires day roll)
ssmd shard apply kalshi-prod --plan split-tier1

# Or manual: move symbols between shards
ssmd shard move kalshi-prod SOLUSD --from tier1 --to tier2
```

## Resharding Workflow

```
┌─────────────────────────────────────────────────────────────────────┐
│                        RESHARDING WORKFLOW                           │
└───────────────────────────────────────────────────────────────────────┘

1. Plan Phase (ssmd shard plan)
   ├── Analyze current load distribution
   ├── Propose new shard boundaries
   └── Estimate impact

2. Validate Phase (ssmd shard validate)
   ├── Check no symbol orphans
   ├── Verify resource availability
   └── Confirm NATS subject mappings

3. Apply Phase (during day roll)
   ├── End current day (normal teardown)
   ├── Update shard definitions in environment YAML
   ├── Update NATS mirror config
   ├── Start new day (connectors read new assignments)
   └── Verify data flowing correctly

4. Rollback (if needed)
   ├── Restore previous shard definitions (git revert)
   └── Roll back NATS config
```

## CLI Commands

```bash
# List shards
ssmd shard list kalshi-prod

# Show shard details
ssmd shard show kalshi-prod tier1
# Shard: tier1
# Selector: tier=1
# Symbols: 5 (BTCUSD, ETHUSD, SOLUSD, AVAXUSD, MATICUSD)
# Replicas: 2 (current) / 3 (max)
# Resources: 500m CPU, 512Mi memory
# Message Rate: 45,000/s
# Status: healthy

# Show symbol → shard mapping
ssmd shard symbols kalshi-prod
# SYMBOL          SHARD    MSG/S
# BTCUSD          tier1    15,000
# ETHUSD          tier1    12,000
# INXD-25-B4000   tier2    500

# Move symbol between shards
ssmd shard move kalshi-prod SOLUSD --from tier1 --to tier2

# Create new shard
ssmd shard create kalshi-prod tier1a --selector 'tier=1,prefix=BTC*'

# Plan resharding
ssmd shard plan kalshi-prod --strategy rebalance

# Apply resharding plan
ssmd shard apply kalshi-prod --plan <plan-id>
```

## Fixed Memory Profile

All components run with bounded memory (per design requirement):

```yaml
# Resource limits enforced
resources:
  requests:
    cpu: "250m"
    memory: "256Mi"
  limits:
    cpu: "500m"
    memory: "512Mi"  # Hard limit - OOMKilled if exceeded
```

Components designed for fixed memory:
- **Bounded buffers** - Drop policy when full, not grow
- **Streaming writes** - Don't buffer full day in memory
- **LRU caches** - Evict oldest when at capacity
- **Batch processing** - Process in chunks, not all at once

```rust
// Example: Fixed-size buffer
pub struct FixedBuffer<T> {
    items: VecDeque<T>,
    capacity: usize,
    drop_policy: DropPolicy,
}

impl<T> FixedBuffer<T> {
    pub fn push(&mut self, item: T) -> Option<T> {
        if self.items.len() >= self.capacity {
            match self.drop_policy {
                DropPolicy::DropOldest => self.items.pop_front(),
                DropPolicy::DropNewest => return Some(item),
                DropPolicy::Block => panic!("Buffer full"),
            };
        }
        self.items.push_back(item);
        None
    }
}
```

## Shard State (No Database)

Shard assignments are part of the environment YAML (GitOps). Runtime metrics are in cache:

```
# Redis keys for shard metrics
{env}:shard:{shard}:symbols      # Set of assigned symbols
{env}:shard:{shard}:replicas     # Current replica count
{env}:shard:{shard}:metrics      # Message rate, CPU, memory
```

Resharding history is tracked in git via environment YAML changes.
