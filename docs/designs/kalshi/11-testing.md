# ssmd: Kalshi Design - Testing Strategy

Testing ensures correctness without a QA team. The system tests itself through automation, replay, and comparison.

## Testing Layers

```
┌─────────────────────────────────────────────────────────────────────┐
│                        PRODUCTION                                    │
│   Real feeds, real data, real users                                 │
└─────────────────────────────────────────────────────────────────────┘
                              ▲
┌─────────────────────────────────────────────────────────────────────┐
│                     REPLAY TESTING                                   │
│   Historical data, production code, automated comparison            │
└─────────────────────────────────────────────────────────────────────┘
                              ▲
┌─────────────────────────────────────────────────────────────────────┐
│                   INTEGRATION TESTING                                │
│   In-memory middleware, real components, docker-compose             │
└─────────────────────────────────────────────────────────────────────┘
                              ▲
┌─────────────────────────────────────────────────────────────────────┐
│                      UNIT TESTING                                    │
│   Isolated functions, mocked dependencies, fast feedback            │
└─────────────────────────────────────────────────────────────────────┘
```

## Unit Tests

Fast, isolated tests for individual functions:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kalshi_trade() {
        let raw = r#"{"type":"trade","ticker":"INXD-25-B4000","price":0.45,"count":10}"#;
        let trade = parse_kalshi_message(raw).unwrap();

        assert!(matches!(trade, KalshiMessage::Trade(_)));
        if let KalshiMessage::Trade(t) = trade {
            assert_eq!(t.ticker, "INXD-25-B4000");
            assert_eq!(t.price, 0.45);
        }
    }

    #[test]
    fn test_capnp_roundtrip() {
        let trade = Trade {
            timestamp: 1702540800000,
            ticker: "BTCUSD".into(),
            price: 45000.0,
            size: 100,
            side: Side::Buy,
            trade_id: "abc123".into(),
        };

        let encoded = trade.to_capnp();
        let decoded = Trade::from_capnp(&encoded).unwrap();

        assert_eq!(trade, decoded);
    }

    #[test]
    fn test_retry_policy_backoff() {
        let policy = RetryPolicy::default();
        let delays: Vec<_> = (0..5).map(|i| policy.delay_for_attempt(i)).collect();

        // Should be exponential with jitter
        assert!(delays[1] > delays[0]);
        assert!(delays[2] > delays[1]);
        assert!(delays[4] <= policy.max_delay);
    }
}
```

## Integration Tests

Tests with real components but in-memory middleware:

```rust
#[tokio::test]
async fn test_connector_to_gateway_flow() {
    // Setup in-memory middleware
    let transport = Arc::new(InMemoryTransport::new());
    let storage = Arc::new(InMemoryStorage::new());

    // Create components
    let connector = Connector::new(
        MockKalshiClient::new(sample_messages()),
        transport.clone(),
    );
    let gateway = Gateway::new(transport.clone());

    // Start components
    let connector_handle = tokio::spawn(connector.run());
    let gateway_handle = tokio::spawn(gateway.run());

    // Connect a test client
    let mut client = gateway.connect_test_client().await;
    client.subscribe(&["INXD-25-B4000"]).await;

    // Wait for messages to flow
    let msg = timeout(Duration::from_secs(5), client.next()).await.unwrap();

    assert!(matches!(msg, GatewayMessage::Trade(_)));

    // Cleanup
    connector_handle.abort();
    gateway_handle.abort();
}

#[tokio::test]
async fn test_archiver_writes_to_storage() {
    let transport = Arc::new(InMemoryTransport::new());
    let storage = Arc::new(InMemoryStorage::new());

    // Publish test messages
    for i in 0..100 {
        transport.publish("kalshi.trade.BTCUSD", sample_trade(i)).await;
    }

    // Run archiver
    let archiver = Archiver::new(transport.clone(), storage.clone());
    archiver.flush().await;

    // Verify storage
    let files = storage.list("ssmd-raw", "kalshi/").await.unwrap();
    assert!(!files.is_empty());

    let content = storage.get("ssmd-raw", &files[0].key).await.unwrap();
    assert!(content.len() > 0);
}
```

## Docker Compose for Local Integration

```yaml
# docker-compose.test.yaml
version: '3.8'

services:
  nats:
    image: nats:2.10
    command: ["--jetstream"]
    ports:
      - "4222:4222"

  minio:
    image: minio/minio
    command: server /data
    environment:
      MINIO_ROOT_USER: minioadmin
      MINIO_ROOT_PASSWORD: minioadmin
    ports:
      - "9000:9000"

  redis:
    image: redis:7
    ports:
      - "6379:6379"
```

```bash
# Run integration tests
docker-compose -f docker-compose.test.yaml up -d
cargo test --features integration
docker-compose -f docker-compose.test.yaml down
```

## Replay Testing

Compare new code against recorded production data:

```rust
pub struct ReplayTest {
    date: NaiveDate,
    feed: String,
    baseline_version: String,
    candidate_version: String,
}

impl ReplayTest {
    pub async fn run(&self) -> ReplayReport {
        // Load raw data from storage
        let raw_data = self.load_raw_data().await;

        // Process with baseline version
        let baseline_output = self.process_with_version(&self.baseline_version, &raw_data).await;

        // Process with candidate version
        let candidate_output = self.process_with_version(&self.candidate_version, &raw_data).await;

        // Compare outputs
        let diff = self.compare_outputs(&baseline_output, &candidate_output);

        ReplayReport {
            date: self.date,
            feed: self.feed.clone(),
            baseline_count: baseline_output.len(),
            candidate_count: candidate_output.len(),
            differences: diff,
            passed: diff.is_empty(),
        }
    }
}
```

CLI for replay testing:

```bash
# Replay single day
ssmd test replay --feed kalshi --date 2025-12-14 \
  --baseline v1.2.3 --candidate v1.2.4

# Replay date range
ssmd test replay --feed kalshi \
  --from 2025-12-01 --to 2025-12-14 \
  --baseline v1.2.3 --candidate v1.2.4

# Output
Replay Test Report
==================
Feed: kalshi
Date Range: 2025-12-01 to 2025-12-14
Baseline: v1.2.3
Candidate: v1.2.4

Date        Baseline    Candidate   Status
2025-12-01  1,234,567   1,234,567   PASS
2025-12-02  1,245,678   1,245,678   PASS
2025-12-03  1,256,789   1,256,792   FAIL (3 diffs)
```

## Non-Realtime Clock (Backtesting)

For backtesting and replay, components support a non-realtime clock:

```rust
/// Clock abstraction for time-dependent operations
pub trait Clock: Send + Sync {
    fn now(&self) -> u64;  // Unix nanos
    fn advance(&self, nanos: u64);
}

/// Real-time clock for production
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    fn advance(&self, _nanos: u64) {
        // No-op for real clock
    }
}

/// Controllable clock for backtesting
pub struct SimulatedClock {
    current: AtomicU64,
}

impl Clock for SimulatedClock {
    fn now(&self) -> u64 {
        self.current.load(Ordering::SeqCst)
    }

    fn advance(&self, nanos: u64) {
        self.current.fetch_add(nanos, Ordering::SeqCst);
    }
}
```

Backtesting workflow:

```bash
# Run backtest with simulated clock
ssmd backtest --feed kalshi --date 2025-12-14 \
  --strategy my_strategy.yaml \
  --speed 10x  # 10x faster than realtime

# Or step-through mode for debugging
ssmd backtest --feed kalshi --date 2025-12-14 \
  --strategy my_strategy.yaml \
  --step  # Manual clock advancement
```

## Automated QA Pipeline

GitHub Actions workflow for continuous testing:

```yaml
# .github/workflows/test.yaml
name: Test

on:
  push:
    branches: [main]
  pull_request:

jobs:
  unit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --lib

  integration:
    runs-on: ubuntu-latest
    services:
      nats:
        image: nats:2.10
        options: --health-cmd "nats-server --help" --health-interval 10s
      redis:
        image: redis:7
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --features integration

  replay:
    runs-on: ubuntu-latest
    if: github.event_name == 'pull_request'
    steps:
      - uses: actions/checkout@v4
      - name: Run replay tests
        run: |
          ssmd test replay --feed kalshi --date $(date -d "yesterday" +%Y-%m-%d) \
            --baseline ${{ github.base_ref }} \
            --candidate ${{ github.head_ref }}
```

## Environment Comparison

Automatically spin up environments to compare versions:

```bash
# Compare two versions side-by-side
ssmd test compare \
  --env-a kalshi-v1.2.3 \
  --env-b kalshi-v1.2.4 \
  --duration 1h \
  --feed kalshi

# Outputs
Comparison Report
=================
Duration: 1 hour
Feed: kalshi

Metric              v1.2.3      v1.2.4      Diff
Messages received   45,678      45,678      0
Messages published  45,670      45,672      +2
Avg latency (ms)    2.3         2.1         -8.7%
Memory usage (MB)   256         248         -3.1%
CPU usage (%)       15          14          -6.7%
Errors              0           0           0

Result: PASS - no regressions detected
```
