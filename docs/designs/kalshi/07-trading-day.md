# ssmd: Kalshi Design - Trading Day Management

Trading day is a first-class concept. Data is partitioned by it, operations are scheduled around it, and the system state is tied to it.

## Trading Day Concept

```
                    Trading Day 2025-12-14 (UTC)
    ┌─────────────────────────────────────────────────────────┐
    │                                                         │
00:10                                                      00:00
START ──────────────────────────────────────────────────▶ END
    │                                                         │
    │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐   │
    │  │ Sync    │  │Capture  │  │ Stream  │  │ Archive │   │
    │  │SecMaster│─▶│  Data   │─▶│Clients  │─▶│   EOD   │   │
    │  └─────────┘  └─────────┘  └─────────┘  └─────────┘   │
    │                                                         │
    └─────────────────────────────────────────────────────────┘
                              │
                              ▼ ROLL
    ┌─────────────────────────────────────────────────────────┐
    │              Trading Day 2025-12-15 (UTC)               │
    └─────────────────────────────────────────────────────────┘
```

## Trading Day States

```
         ┌──────────────────────────────────────────────────────────┐
         │                                                          │
         ▼                                                          │
    ┌─────────┐     ┌─────────┐     ┌─────────┐     ┌─────────┐   │
    │ PENDING │────▶│ STARTING│────▶│  ACTIVE │────▶│ ENDING  │───┘
    └─────────┘     └─────────┘     └─────────┘     └─────────┘
         │               │               │               │
         │               │               │               ▼
         │               │               │          ┌─────────┐
         │               ▼               ▼          │ COMPLETE│
         │          ┌─────────┐    ┌─────────┐     └─────────┘
         └─────────▶│  FAILED │◀───│  ERROR  │
                    └─────────┘    └─────────┘
```

| State | Description |
|-------|-------------|
| `PENDING` | Day defined but not started |
| `STARTING` | Startup workflow running (sync, connect, health check) |
| `ACTIVE` | Day is live, data flowing |
| `ENDING` | Teardown workflow running (drain, flush, verify) |
| `COMPLETE` | Day ended successfully, data archived |
| `ERROR` | Error during active day (can retry) |
| `FAILED` | Startup/teardown failed (needs intervention) |

## State Storage (No Database)

Trading day state is stored in cache (Redis) and journal (NATS):

```
# Redis keys
{env}:day:current              # Current trading day date
{env}:day:{date}:state         # Day state (pending, active, etc.)
{env}:day:{date}:stats         # Day statistics JSON
{env}:day:{date}:start_time    # When day started
{env}:day:{date}:end_time      # When day ended

# NATS journal
{env}.day.events               # State transitions and events
```

## CLI Commands

```bash
# View current trading day status
ssmd day status
# Environment: kalshi-prod
# Trading Day: 2025-12-14
# State: ACTIVE
# Started: 2025-12-14T00:10:00Z (14h 30m ago)
# Messages: 1,234,567
# Gaps: 0

# Start a new trading day
ssmd day start kalshi-prod
# Starting trading day 2025-12-14...
#   ✓ Syncing security master
#   ✓ Starting connector
#   ✓ Starting archiver
#   ✓ Starting gateway
#   ✓ Health check passed
# Trading day 2025-12-14 is ACTIVE

# Start a specific date (for replay/backfill)
ssmd day start kalshi-prod --date 2025-12-10

# End the current trading day
ssmd day end kalshi-prod
# Ending trading day 2025-12-14...
#   ✓ Draining gateway connections
#   ✓ Flushing archiver buffers
#   ✓ Stopping connector
#   ✓ Verifying archive completeness
#   ✓ Recording day completion
# Trading day 2025-12-14 is COMPLETE

# Roll to next day (end current + start next)
ssmd day roll kalshi-prod
# Rolling from 2025-12-14 to 2025-12-15...
#   Ending 2025-12-14...
#   ✓ Day 2025-12-14 COMPLETE
#   Starting 2025-12-15...
#   ✓ Day 2025-12-15 ACTIVE
# Roll complete

# Force end (skip verification, for emergencies)
ssmd day end kalshi-prod --force

# View trading day history (from journal)
ssmd day history kalshi-prod --limit 7
# DATE        STATE     START               END                 MSGS       GAPS
# 2025-12-14  ACTIVE    2025-12-14T00:10   -                   1,234,567  0
# 2025-12-13  COMPLETE  2025-12-13T00:10   2025-12-14T00:00    2,345,678  0
# 2025-12-12  COMPLETE  2025-12-12T00:10   2025-12-13T00:00    1,987,654  2

# View specific day details
ssmd day show kalshi-prod 2025-12-12
# Trading Day: 2025-12-12
# Environment: kalshi-prod
# State: COMPLETE
# Started: 2025-12-12T00:10:00Z
# Ended: 2025-12-13T00:00:00Z
# Duration: 23h 50m
# Messages: 1,987,654
# Gaps: 2
#   - 14:30:00 - 14:32:15 (connection_lost)
#   - 18:45:30 - 18:45:45 (rate_limited)
# Archive: s3://ssmd-raw/kalshi/2025/12/12/
```

## Temporal Workflows

```go
// StartTradingDay workflow
func StartTradingDay(ctx workflow.Context, env string, date time.Time) error {
    logger := workflow.GetLogger(ctx)
    logger.Info("Starting trading day", "env", env, "date", date)

    // Update state: PENDING -> STARTING
    err := workflow.ExecuteActivity(ctx, UpdateDayState, env, date, "starting").Get(ctx, nil)
    if err != nil {
        return err
    }

    // 1. Sync security master
    err = workflow.ExecuteActivity(ctx, SyncSecurityMaster, env).Get(ctx, nil)
    if err != nil {
        workflow.ExecuteActivity(ctx, UpdateDayState, env, date, "failed").Get(ctx, nil)
        return fmt.Errorf("sync security master: %w", err)
    }

    // 2. Start connector
    err = workflow.ExecuteActivity(ctx, StartConnector, env, date).Get(ctx, nil)
    if err != nil {
        workflow.ExecuteActivity(ctx, UpdateDayState, env, date, "failed").Get(ctx, nil)
        return fmt.Errorf("start connector: %w", err)
    }

    // 3. Start archiver
    err = workflow.ExecuteActivity(ctx, StartArchiver, env, date).Get(ctx, nil)
    if err != nil {
        workflow.ExecuteActivity(ctx, UpdateDayState, env, date, "failed").Get(ctx, nil)
        return fmt.Errorf("start archiver: %w", err)
    }

    // 4. Start gateway
    err = workflow.ExecuteActivity(ctx, StartGateway, env).Get(ctx, nil)
    if err != nil {
        workflow.ExecuteActivity(ctx, UpdateDayState, env, date, "failed").Get(ctx, nil)
        return fmt.Errorf("start gateway: %w", err)
    }

    // 5. Health check
    err = workflow.ExecuteActivity(ctx, HealthCheck, env).Get(ctx, nil)
    if err != nil {
        workflow.ExecuteActivity(ctx, UpdateDayState, env, date, "failed").Get(ctx, nil)
        return fmt.Errorf("health check: %w", err)
    }

    // Update state: STARTING -> ACTIVE
    err = workflow.ExecuteActivity(ctx, UpdateDayState, env, date, "active").Get(ctx, nil)
    if err != nil {
        return err
    }

    logger.Info("Trading day started successfully", "env", env, "date", date)
    return nil
}

// EndTradingDay workflow
func EndTradingDay(ctx workflow.Context, env string, date time.Time, force bool) error {
    // ... similar pattern with drain, flush, verify, complete
}

// RollTradingDay workflow
func RollTradingDay(ctx workflow.Context, env string) error {
    // Get current day, end it, start next
}
```

## Scheduled Operations

Trading day operations can be scheduled via Temporal:

```yaml
# exchanges/environments/kalshi-prod.yaml
name: kalshi-prod
feed: kalshi
schema: trade:v1

schedule:
  timezone: UTC
  day_start: "00:10"      # When to start each day
  day_end: "00:00"        # When to end each day (next calendar day)
  auto_roll: true         # Automatically roll at day_end
```

## Data Partitioning

All data is partitioned by trading day:

```
ssmd-raw/
  kalshi/
    2025/12/14/           # Trading day partition
      trades-00.jsonl.zst
      trades-01.jsonl.zst
      orderbook-00.jsonl.zst

ssmd-normalized/
  kalshi/
    v1/
      trade/
        2025/12/14/       # Trading day partition
          INXD-25-B4000/
            data.capnp.zst
```

Components receive trading day at startup:

```rust
pub struct ConnectorConfig {
    pub environment: String,
    pub trading_day: NaiveDate,  // Partitions data to correct location
    pub feed: FeedConfig,
}

impl Connector {
    pub fn archive_path(&self) -> String {
        format!(
            "{}/{}/{:04}/{:02}/{:02}/",
            self.config.storage.raw_bucket,
            self.config.feed.name,
            self.config.trading_day.year(),
            self.config.trading_day.month(),
            self.config.trading_day.day()
        )
    }
}
```

## Recovery Scenarios

```bash
# Day failed to start - retry
ssmd day start kalshi-prod --date 2025-12-14

# Day ended with errors - review and complete manually
ssmd day show kalshi-prod 2025-12-14
ssmd day end kalshi-prod --force

# Missed a day - backfill
ssmd day start kalshi-prod --date 2025-12-12 --mode replay
ssmd day end kalshi-prod --date 2025-12-12

# System crash mid-day - resume
ssmd day status kalshi-prod
# State: ERROR
ssmd day recover kalshi-prod
# Attempts to resume from last checkpoint
```

## Day History (Journal-based)

Trading day history is reconstructed from the journal:

```go
func (s *DayService) History(env string, limit int) ([]TradingDay, error) {
    // Read from journal: {env}.day.events
    events, err := s.journal.Read(fmt.Sprintf("%s.day.events", env), limit*10)
    if err != nil {
        return nil, err
    }

    // Reconstruct day states from events
    days := make(map[string]*TradingDay)
    for _, event := range events {
        date := event.Date
        if days[date] == nil {
            days[date] = &TradingDay{Date: date}
        }
        days[date].Apply(event)
    }

    // Sort by date descending, return top N
    return sortAndLimit(days, limit), nil
}
```

This provides:
- Full audit trail of state transitions
- Replay capability for debugging
- No database required for history
