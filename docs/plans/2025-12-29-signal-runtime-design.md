# Signal Runtime Design

**Date**: 2025-12-29
**Status**: Draft
**Branch**: `feature/signal-runtime`

## Overview

The signal runtime is a standalone process that runs a single signal against real-time market data from NATS, publishing fires back to NATS for downstream consumption.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Signal Runtime                          │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │ RecordSource │───▶│ StateBuilders│───▶│   Signal     │  │
│  │   (NATS)     │    │  (per-ticker)│    │  evaluate()  │  │
│  └──────────────┘    └──────────────┘    └──────┬───────┘  │
│                                                  │ fire?    │
│                                           ┌──────▼───────┐  │
│                                           │   FireSink   │  │
│                                           │    (NATS)    │  │
│                                           └──────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

**Key decisions:**
- Subscribe to all tickers (`prod.kalshi.>`), maintain per-ticker state
- Publish fires to `signals.<signal-id>.fires` subject
- Single signal per process (K8s-style scaling)
- Abstract interfaces for input/output (testable, swappable)

## Interfaces

### RecordSource

Abstracts where market data comes from:

```typescript
// src/runtime/interfaces.ts
interface RecordSource {
  subscribe(): AsyncIterable<MarketRecord>;
  close(): Promise<void>;
}
```

**Implementations:**
- `NatsRecordSource` - connects to NATS JetStream, subscribes to stream
- `FileRecordSource` - reads JSONL.gz files (reuses backtest code)

### FireSink

Abstracts where signal fires go:

```typescript
interface SignalFire {
  signalId: string;
  ts: number;
  ticker: string;
  payload: unknown;
}

interface FireSink {
  publish(fire: SignalFire): Promise<void>;
  close(): Promise<void>;
}
```

**Implementations:**
- `NatsFireSink` - publishes to `signals.<signal-id>.fires`
- `ConsoleFirSink` - logs to stdout (for testing)

## Runtime Core

The runner orchestrates the data flow:

```typescript
// src/runtime/runner.ts
interface RuntimeConfig {
  signalPath: string;
  source: RecordSource;
  sink: FireSink;
  stateConfig?: Record<string, Record<string, unknown>>;
}

async function runSignal(config: RuntimeConfig): Promise<void> {
  const signal = await loadSignal(config.signalPath);
  const signalModule = await compileSignal(signal);

  // State builders per ticker (reuse from backtest)
  const tickerBuilders = new Map<string, Map<string, StateBuilder>>();

  for await (const record of config.source.subscribe()) {
    const builders = getOrCreateBuilders(tickerBuilders, record.ticker, signal.requires);

    for (const builder of builders.values()) {
      builder.update(record);
    }

    const state = buildStateMap(builders);
    if (signalModule.evaluate(state)) {
      await config.sink.publish({
        signalId: signal.id,
        ts: record.ts,
        ticker: record.ticker,
        payload: signalModule.payload(state),
      });
    }
  }
}
```

## NATS Implementation

### NatsRecordSource

```typescript
// src/runtime/nats.ts
class NatsRecordSource implements RecordSource {
  constructor(
    private servers: string,           // "nats://nats.nats.svc:4222"
    private stream: string,            // "PROD_KALSHI"
    private subject: string,           // "prod.kalshi.>"
    private consumerName?: string,     // Optional durable consumer
  ) {}

  async *subscribe(): AsyncIterable<MarketRecord> {
    this.nc = await connect({ servers: this.servers });
    this.js = this.nc.jetstream();

    const consumer = await this.js.consumers.get(this.stream, this.consumerName);
    const messages = await consumer.consume();

    for await (const msg of messages) {
      const raw = JSON.parse(msg.data);
      const record = parseRecord(raw);
      if (record) yield record;
      msg.ack();
    }
  }
}
```

### NatsFireSink

```typescript
class NatsFireSink implements FireSink {
  async publish(fire: SignalFire): Promise<void> {
    if (!this.nc) {
      this.nc = await connect({ servers: this.servers });
    }
    const subject = `signals.${fire.signalId}.fires`;
    this.nc.publish(subject, JSON.stringify(fire));
  }
}
```

## CLI Commands

### signal run

Run a signal against live or file data:

```bash
# Live NATS
deno task cli signal run volume-1m-30min

# Local files
deno task cli signal run volume-1m-30min --source file --data ./data
```

### signal subscribe

Watch signal fires:

```bash
deno task cli signal subscribe volume-1m-30min

# Output:
# 2025-12-29T15:30:45Z KXBTC-25DEC29
#   {"dollarVolume":1234567,"contractVolume":2469134}
```

### signal list

List available signals:

```bash
deno task cli signal list

# Output:
# volume-1m-30min    Volume Crosses $1M in 30 Minutes
```

## File Structure

```
ssmd-agent/src/runtime/
├── interfaces.ts      # RecordSource, FireSink, SignalFire types
├── runner.ts          # Core runSignal() orchestration
├── nats.ts            # NatsRecordSource, NatsFireSink
└── file.ts            # FileRecordSource (reuse backtest code)

ssmd-agent/src/cli/commands/signal.ts  # run, subscribe, list commands
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NATS_URL` | `nats://localhost:4222` | NATS server URL |
| `NATS_STREAM` | `PROD_KALSHI` | JetStream stream name |
| `NATS_SUBJECT` | `prod.kalshi.>` | Subject filter |

## Testing

```bash
# 1. Local files (no NATS)
deno task cli signal run volume-1m-30min --source file --data ./data

# 2. Port-forward NATS
kubectl port-forward -n nats svc/nats 4222:4222
NATS_URL=nats://localhost:4222 deno task cli signal run volume-1m-30min

# 3. Watch fires
NATS_URL=nats://localhost:4222 deno task cli signal subscribe volume-1m-30min
```

## Future: K8s Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: signal-volume-1m-30min
  namespace: ssmd
spec:
  replicas: 1
  template:
    spec:
      containers:
      - name: runtime
        image: ghcr.io/aaronwald/ssmd-cli-ts:latest
        args: ["signal", "run", "volume-1m-30min"]
        env:
        - name: NATS_URL
          value: "nats://nats.nats.svc:4222"
```

## Implementation Plan

1. Create `src/runtime/interfaces.ts` with types
2. Create `src/runtime/runner.ts` with core loop
3. Create `src/runtime/nats.ts` with NATS implementations
4. Create `src/runtime/file.ts` extracting from backtest
5. Add `src/cli/commands/signal.ts` with run/subscribe/list
6. Test locally with file source
7. Test with port-forwarded NATS
8. Deploy to cluster
