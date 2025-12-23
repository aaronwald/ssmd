# ssmd: Kalshi Design - Security Master

Security master (secmaster) stores market/instrument metadata for each feed. Essential for prediction markets where contracts expire.

## Design (No Database)

Unlike feed/schema/environment metadata (which uses GitOps), security master is **runtime data** that changes frequently. Markets are created, open, close, and settle throughout the day.

The approach is:
1. Sync job fetches market data from Kalshi API
2. Store in cache (Redis) for fast lookups
3. Publish changes to journal (NATS) for downstream consumers
4. No database required

## Data Model

```rust
pub struct Market {
    pub ticker: String,
    pub kalshi_id: String,
    pub title: String,
    pub category: Option<String>,
    pub status: MarketStatus,

    // Contract timing
    pub open_time: Option<DateTime<Utc>>,
    pub close_time: Option<DateTime<Utc>>,
    pub expiration_time: Option<DateTime<Utc>>,
    pub settlement_time: Option<DateTime<Utc>>,

    // Settlement (if resolved)
    pub result: Option<SettlementResult>,  // yes, no
    pub settled_at: Option<DateTime<Utc>>,

    // Metadata
    pub updated_at: DateTime<Utc>,
    pub raw_metadata: serde_json::Value,
}

pub enum MarketStatus {
    Active,
    Closed,
    Settled,
}

pub enum SettlementResult {
    Yes,
    No,
}
```

## Cache Layout

Markets are stored in Redis with environment prefix:

```
{env}:secmaster:markets             # Hash of ticker -> Market JSON
{env}:secmaster:markets:by_category # Hash of category -> Set<ticker>
{env}:secmaster:markets:expiring    # Sorted set by expiration time
{env}:secmaster:last_sync           # Timestamp of last sync
```

**Examples:**

```bash
# Get a market
HGET kalshi-prod:secmaster:markets INXD-25-B4000

# Get all markets in category
SMEMBERS kalshi-prod:secmaster:markets:by_category:politics

# Get markets expiring in next 24h
ZRANGEBYSCORE kalshi-prod:secmaster:markets:expiring 0 {now+24h}
```

## Sync Job

Temporal workflow syncs markets from Kalshi API:

```rust
pub async fn sync_markets(ctx: &WorkflowContext) -> Result<SyncResult, Error> {
    // 1. Fetch all markets from Kalshi API
    let markets = ctx.activity(FetchKalshiMarkets).await?;

    // 2. Get current state from cache
    let current = ctx.activity(GetCachedMarkets).await?;

    // 3. Compute changes
    let changes = compute_changes(&current, &markets);

    // 4. Update cache
    ctx.activity(UpdateCache { markets: &markets }).await?;

    // 5. Publish changes to journal
    for change in &changes {
        ctx.activity(PublishChange { change }).await?;
    }

    Ok(SyncResult {
        total: markets.len(),
        added: changes.iter().filter(|c| c.is_add()).count(),
        updated: changes.iter().filter(|c| c.is_update()).count(),
        removed: changes.iter().filter(|c| c.is_remove()).count(),
    })
}
```

**Schedule:**
- Full sync: Daily at market open
- Incremental sync: Every 5 minutes during trading hours
- On-demand: Via CLI or webhook

## Change Journal

Market changes are published to NATS for downstream consumers:

```
Subject: {env}.secmaster.changes

{
  "type": "market_added",
  "ticker": "INXD-25-B4000",
  "market": { ... },
  "timestamp": "2025-12-14T10:00:00Z"
}

{
  "type": "market_updated",
  "ticker": "INXD-25-B4000",
  "changes": {
    "status": { "old": "active", "new": "closed" }
  },
  "timestamp": "2025-12-14T16:00:00Z"
}

{
  "type": "market_settled",
  "ticker": "INXD-25-B4000",
  "result": "yes",
  "timestamp": "2025-12-14T16:05:00Z"
}
```

Consumers (connectors, gateways, agents) subscribe to this stream to react to changes in real-time.

## CLI Commands

```bash
# Trigger manual sync
ssmd secmaster sync kalshi-prod

# List markets
ssmd secmaster list kalshi-prod
ssmd secmaster list kalshi-prod --category politics
ssmd secmaster list kalshi-prod --expiring-within 24h

# Show market details
ssmd secmaster show kalshi-prod INXD-25-B4000

# Search markets
ssmd secmaster search kalshi-prod "bitcoin"

# Export markets (for backup/analysis)
ssmd secmaster export kalshi-prod > markets.json
```

## Connector Integration

Connectors use secmaster to:
1. Validate subscribed symbols exist
2. Check market status before subscribing
3. Handle market transitions (close, settle)

```rust
impl Connector {
    pub async fn subscribe(&self, ticker: &str) -> Result<(), Error> {
        // Check market exists and is active
        let market = self.secmaster.get(ticker).await?;
        match market {
            None => Err(Error::MarketNotFound(ticker.to_string())),
            Some(m) if m.status != MarketStatus::Active => {
                Err(Error::MarketNotActive(ticker.to_string(), m.status))
            }
            Some(_) => {
                self.websocket.subscribe(ticker).await
            }
        }
    }
}
```

## Expiration Handling

Markets in prediction exchanges expire. The system handles this:

1. **Sync job** updates market status when markets close/settle
2. **Connectors** receive change events and unsubscribe from settled markets
3. **Archiver** finalizes data for expired markets
4. **Gateway** stops streaming settled markets

## Cache Warming

On startup, components warm their local caches:

```rust
impl Gateway {
    pub async fn start(&self) -> Result<(), Error> {
        // Warm cache from Redis
        let markets = self.secmaster.get_all().await?;
        for market in markets {
            self.local_cache.insert(market.ticker.clone(), market);
        }

        // Subscribe to changes
        let mut changes = self.journal.subscribe("secmaster.changes").await?;
        tokio::spawn(async move {
            while let Some(change) = changes.next().await {
                self.apply_change(change).await;
            }
        });

        Ok(())
    }
}
```

## No Database Benefits

By using cache + journal instead of database:
- **Simpler operations** - No schema migrations, backups, replicas
- **Faster lookups** - Redis is in-memory
- **Event-driven** - Changes propagate via journal, not polling
- **Ephemeral** - Rebuild from Kalshi API anytime (source of truth is the exchange)
