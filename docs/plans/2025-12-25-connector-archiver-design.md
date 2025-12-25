# Connector NATS-Only & Archiver Design

**Date:** 2025-12-25
**Status:** Approved
**Branch:** `feature/connector-nats-only-archiver`

## Overview

Refactor the connector to output NATS-only (remove file writer path) and create a separate archiver service that subscribes to NATS and persists data to disk.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Connector output | NATS-only | Single responsibility, enables sharding |
| Format | Raw JSON passthrough | Simple, preserves all fields, agent tools parse at query time |
| Subject pattern | `{env}.{feed}.json.{type}.{ticker}` | Explicit format in subject |
| Cap'n Proto | Removed for MVP | Add back when Signal Runtime needs low-latency |
| Archiver location | New crate `ssmd-archiver` | Separate container, independent versioning |
| Rotation interval | 15 minutes (configurable) | Quick test cycles, production can use 1h/1d |
| Subject caching | No caching for MVP | Allocations cheap at Kalshi volume |

## Future Considerations

### Transform Layer

Raw JSON passthrough is MVP. When adding Polymarket/Kraken, consider a transform service:

```
Raw JSON ──► Transform Service ──► Normalized JSON / Cap'n Proto
             (subscribes)           (publishes)
```

Options:
1. Separate `ssmd-transform` service
2. Archiver writes both raw and normalized
3. Agent tools parse at query time (current MVP)

## Architecture

### Data Flow

```
Kalshi WS ──► Connector ──► NATS JetStream ──► Archiver ──► Local ──► GCS
                │                  │               │
                │                  │               ├── /data/ssmd/kalshi/2025-12-25/
                │                  │               │     1200.jsonl.gz
                │                  │               │     1215.jsonl.gz
                │                  │               │     manifest.json
                │                  │               │
                │                  │               └── gsutil rsync (cron)
                │                  │
                │                  └── Stream: MARKETDATA
                │                      Subject: prod.kalshi.json.trade.INXD-25001
                │
                └── Raw JSON passthrough
```

### Subject Pattern

```
{env}.{feed}.json.{type}.{ticker}

Examples:
  prod.kalshi.json.trade.INXD-25001
  prod.kalshi.json.ticker.KXBTC-25001
  dev.kalshi.json.trade.KXTEST-123
```

## Connector Changes

### Files to Modify

```
ssmd-rust/crates/connector/src/
├── writer.rs          # DELETE FileWriter, keep Writer trait
├── nats_writer.rs     # Simplify: raw JSON passthrough
├── lib.rs             # Remove FileWriter export
└── runner.rs          # Remove file writer creation path

ssmd-rust/crates/middleware/src/nats/
└── subjects.rs        # Add json_trade(), json_ticker() methods
```

### NatsWriter (simplified)

```rust
#[async_trait]
impl Writer for NatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        // Parse just enough to extract message type and ticker
        let ws_msg: WsMessage = serde_json::from_slice(&msg.data)?;

        let subject = match &ws_msg {
            WsMessage::Trade { msg } => self.subjects.json_trade(&msg.market_ticker),
            WsMessage::Ticker { msg } => self.subjects.json_ticker(&msg.market_ticker),
            WsMessage::OrderbookSnapshot { msg } => self.subjects.json_orderbook(&msg.market_ticker),
            WsMessage::OrderbookDelta { msg } => self.subjects.json_orderbook(&msg.market_ticker),
            _ => return Ok(()), // Skip control messages
        };

        // Publish raw bytes - no transformation
        self.transport.publish(&subject, Bytes::from(msg.data.clone())).await?;
        self.message_count += 1;
        Ok(())
    }
}
```

### SubjectBuilder (new methods)

```rust
impl SubjectBuilder {
    pub fn json_trade(&self, ticker: &str) -> String {
        format!("{}.{}.json.trade.{}", self.env, self.feed, ticker)
    }

    pub fn json_ticker(&self, ticker: &str) -> String {
        format!("{}.{}.json.ticker.{}", self.env, self.feed, ticker)
    }

    pub fn json_orderbook(&self, ticker: &str) -> String {
        format!("{}.{}.json.orderbook.{}", self.env, self.feed, ticker)
    }
}
```

## Archiver Design

### Crate Structure

```
ssmd-rust/crates/ssmd-archiver/
├── Cargo.toml
├── src/
│   ├── main.rs           # CLI entry point
│   ├── lib.rs            # Library exports
│   ├── config.rs         # YAML config parsing
│   ├── subscriber.rs     # NATS JetStream consumer
│   ├── writer.rs         # JSONL.gz file writer
│   ├── manifest.rs       # Manifest generation
│   └── rotation.rs       # Time-based file rotation
```

### Configuration

```yaml
# archiver.yaml
nats:
  url: nats://localhost:4222
  stream: MARKETDATA
  consumer: archiver-kalshi
  filter: "prod.kalshi.json.>"

storage:
  path: /data/ssmd

rotation:
  interval: 15m   # 15m for testing, 1d for production
```

### Output Structure

```
/data/ssmd/kalshi/
  2025-12-25/
    1200.jsonl.gz    # 12:00-12:15 UTC
    1215.jsonl.gz    # 12:15-12:30 UTC
    manifest.json
```

### Manifest Format

```json
{
  "feed": "kalshi",
  "date": "2025-12-25",
  "format": "jsonl",
  "rotation_interval": "15m",
  "files": [
    {
      "name": "1200.jsonl.gz",
      "start": "2025-12-25T12:00:00Z",
      "end": "2025-12-25T12:15:00Z",
      "records": 1542,
      "bytes": 204800,
      "nats_start_seq": 100000,
      "nats_end_seq": 101542
    },
    {
      "name": "1215.jsonl.gz",
      "start": "2025-12-25T12:15:00Z",
      "end": "2025-12-25T12:30:00Z",
      "records": 1823,
      "bytes": 245000,
      "nats_start_seq": 101543,
      "nats_end_seq": 103365
    }
  ],
  "gaps": [
    {
      "after_seq": 101000,
      "missing_count": 3,
      "detected_at": "2025-12-25T12:05:32Z"
    }
  ],
  "tickers": ["INXD-25001", "KXBTC-25001"],
  "message_types": ["trade", "ticker"],
  "has_gaps": true
}
```

**Gap detection:** Archiver tracks expected next sequence. When a message arrives with seq > expected, record the gap in manifest.

NATS sequence tracking enables:
- Gap detection (missing sequences)
- Resume from last position on restart
- Replay verification

### Dockerfile

```dockerfile
FROM rust:1.83-slim as builder
WORKDIR /build
COPY . .
RUN cargo build --release --package ssmd-archiver

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/ssmd-archiver /usr/local/bin/
ENTRYPOINT ["ssmd-archiver"]
```

### CI Workflow

New workflow: `.github/workflows/build-archiver.yaml`
- Triggers on `v*` tags
- Builds and pushes `ghcr.io/aaronwald/ssmd-archiver`

## Implementation Order

### Phase 1: Connector Refactor
1. Update `SubjectBuilder` with `json_*` methods
2. Simplify `NatsWriter` to raw JSON passthrough
3. Remove `FileWriter`
4. Update tests
5. Update docs

### Phase 2: Archiver
1. Create `ssmd-archiver` crate
2. Config parsing
3. NATS JetStream subscriber
4. JSONL.gz writer with rotation
5. Manifest generation
6. Graceful shutdown
7. Dockerfile + CI

### Phase 3: Deploy & Test
1. Deploy connector to homelab
2. Deploy archiver to homelab
3. Verify data flow
4. Set up GCS sync cron
5. Test with `ssmd data` commands

## Dependencies

- Connector refactor is independent
- Archiver depends on NATS infrastructure
- `ssmd data` CLI depends on archiver output format
