# Signals

Signals are TypeScript modules that define trading conditions to monitor.

## Directory Structure

```
signals/
├── <signal-name>/
│   ├── signal.ts       # Required: signal logic
│   └── backtest.yaml   # Optional: backtest configuration
```

## Signal Interface

```typescript
// signals/<name>/signal.ts
export const signal = {
  // Unique identifier
  id: "my-signal",

  // Human-readable name (optional)
  name: "My Trading Signal",

  // Required state builders (see available builders below)
  requires: ["volumeProfile"],

  // Evaluate returns true when signal should fire
  evaluate(state: { volumeProfile: VolumeProfileState }): boolean {
    return state.volumeProfile.dollarVolume >= 1_000_000;
  },

  // Payload returns data to include with the fire event
  payload(state: { volumeProfile: VolumeProfileState }) {
    return {
      ticker: state.volumeProfile.ticker,
      dollarVolume: state.volumeProfile.dollarVolume,
    };
  },
};
```

## Available State Builders

### volumeProfile

Tracks volume over a sliding time window.

**State:**
```typescript
interface VolumeProfileState {
  ticker: string;
  totalVolume: number;    // Contract volume in window
  dollarVolume: number;   // USD volume in window
  ratio: number;          // Buy/sell ratio (placeholder)
  average: number;        // Average volume per update
  tradeCount: number;     // Number of updates in window
  lastUpdate: number;     // Unix timestamp (seconds)
  windowMs: number;       // Window size in milliseconds
}
```

**Config (in backtest.yaml):**
```yaml
state:
  volumeProfile:
    windowMs: 1800000  # 30 minutes (default: 300000 = 5 min)
```

## Backtest Manifest

```yaml
# backtest.yaml
feed: kalshi

# Option 1: Date range
date_range:
  from: "2025-12-01"
  to: "2025-12-29"

# Option 2: Explicit dates
dates:
  - "2025-12-28"
  - "2025-12-29"

# Optional: State builder config
state:
  volumeProfile:
    windowMs: 1800000

# Optional: Limit records (for testing)
sample_limit: 10000
```

## Running Backtests

```bash
# From ssmd-agent directory
deno task cli backtest run <signal-name>

# With explicit dates
deno task cli backtest run <signal-name> --dates 2025-12-29

# Allow uncommitted changes
deno task cli backtest run <signal-name> --allow-dirty
```

## Tips

1. **Edge detection**: If you only want to fire once when crossing a threshold,
   track previous state and compare.

2. **Timestamps**: `lastUpdate` is Unix seconds. Convert with `new Date(ts * 1000)`.

3. **Testing**: Use `sample_limit` in backtest.yaml for quick iteration.
