# ssmd: Kalshi Design - Error Handling & Backpressure

Errors are categorized, handled consistently, and surfaced appropriately. The system fails fast on configuration errors and recovers gracefully from transient failures.

## Error Categories

| Category | Examples | Response |
|----------|----------|----------|
| **Configuration** | Invalid YAML, missing secret, unknown feed | Fail fast at startup, don't retry |
| **Transient** | Network timeout, rate limit, connection lost | Retry with backoff, then escalate |
| **Data Quality** | Parse error, unexpected schema, missing field | Log, record in manifest, continue |
| **Fatal** | Out of memory, disk full, auth revoked | Shutdown gracefully, alert |

## Retry Policy

Transient errors use exponential backoff with jitter:

```rust
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub jitter: f64,  // 0.0 to 1.0
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
            jitter: 0.1,
        }
    }
}

pub async fn retry_with_policy<F, T, E>(
    policy: &RetryPolicy,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Future<Output = Result<T, E>>,
    E: IsTransient,
{
    let mut attempt = 0;
    let mut delay = policy.initial_delay;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if e.is_transient() && attempt < policy.max_attempts => {
                attempt += 1;
                let jittered = add_jitter(delay, policy.jitter);
                tokio::time::sleep(jittered).await;
                delay = (delay.mul_f64(policy.multiplier)).min(policy.max_delay);
            }
            Err(e) => return Err(e),
        }
    }
}
```

## Dead Letter Queue

Messages that fail after all retries go to a dead letter queue for inspection:

```rust
pub struct DeadLetter {
    pub original_subject: String,
    pub payload: Bytes,
    pub error: String,
    pub attempts: u32,
    pub first_attempt: u64,
    pub last_attempt: u64,
    pub component: String,  // "connector", "archiver", etc.
}
```

Dead letters are:
1. Published to `{env}.dlq.{component}` NATS subject
2. Recorded in day manifest
3. Visible via `ssmd dlq list` and TUI

```bash
# View dead letters
ssmd dlq list --component connector --since 1h

# Replay a dead letter (after fixing the issue)
ssmd dlq replay --id <dlq-id>

# Purge old dead letters
ssmd dlq purge --older-than 7d
```

## Circuit Breaker

Prevents cascade failures when downstream is unhealthy:

```rust
pub struct CircuitBreaker {
    state: AtomicU8,  // Closed=0, Open=1, HalfOpen=2
    failure_count: AtomicU32,
    success_count: AtomicU32,
    last_failure: AtomicU64,

    // Configuration
    failure_threshold: u32,      // Open after N failures
    success_threshold: u32,      // Close after N successes in half-open
    timeout: Duration,           // Time before half-open
}

impl CircuitBreaker {
    pub async fn call<F, T, E>(&self, operation: F) -> Result<T, CircuitError<E>>
    where
        F: Future<Output = Result<T, E>>,
    {
        match self.state() {
            State::Open => {
                if self.should_try_half_open() {
                    self.set_state(State::HalfOpen);
                } else {
                    return Err(CircuitError::Open);
                }
            }
            _ => {}
        }

        match operation.await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure();
                Err(CircuitError::Failed(e))
            }
        }
    }
}
```

Circuit breakers wrap:
- Exchange WebSocket connections
- NATS publish operations
- S3 storage operations
- Cache operations

## Error Propagation

Errors include context for debugging:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectorError {
    #[error("WebSocket connection failed: {source}")]
    WebSocket {
        #[source]
        source: tungstenite::Error,
        endpoint: String,
        attempt: u32,
    },

    #[error("Failed to parse message: {source}")]
    Parse {
        #[source]
        source: serde_json::Error,
        raw_message: String,
        symbol: Option<String>,
    },

    #[error("Transport publish failed: {source}")]
    Transport {
        #[source]
        source: TransportError,
        subject: String,
    },

    #[error("Configuration error: {message}")]
    Config { message: String },
}

impl ConnectorError {
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::WebSocket { .. } | Self::Transport { .. })
    }
}
```

## Graceful Degradation

When non-critical components fail:

| Component Failure | Degradation |
|-------------------|-------------|
| Cache unavailable | Bypass cache, query source directly (slower) |
| Archiver behind | Continue streaming, archiver catches up |
| One symbol fails | Continue other symbols, log gap |

## Alerting Integration

Errors surface as metrics and alerts:

```yaml
# Error rate alert
- alert: HighErrorRate
  expr: rate(ssmd_errors_total[5m]) > 0.1
  for: 2m
  labels:
    severity: warning
  annotations:
    summary: "High error rate in {{ $labels.component }}"

# Circuit breaker open
- alert: CircuitBreakerOpen
  expr: ssmd_circuit_breaker_state == 1
  for: 1m
  labels:
    severity: critical
  annotations:
    summary: "Circuit breaker open for {{ $labels.target }}"

# Dead letters accumulating
- alert: DeadLettersAccumulating
  expr: increase(ssmd_dead_letters_total[1h]) > 100
  labels:
    severity: warning
```

---

# Backpressure & Slow Consumers

The system handles consumers that can't keep up without losing data or blocking producers.

## Design Principles

1. **Never block producers** - Connector must keep ingesting exchange data
2. **Bound memory** - Per-client buffers have limits
3. **Detect early** - Identify slow consumers before they cause problems
4. **Degrade gracefully** - Slow consumers get dropped, not crashed

## Architecture

```
                                    ┌─────────────────┐
                                    │  Fast Client A  │◀── Full stream
                                    └─────────────────┘
┌───────────┐     ┌──────────┐     ┌─────────────────┐
│ Connector │────▶│   NATS   │────▶│  Slow Client B  │◀── Buffered, then dropped
└───────────┘     │ JetStream│     └─────────────────┘
                  └──────────┘     ┌─────────────────┐
                                   │  Client C (sub) │◀── Conflated snapshots
                                   └─────────────────┘
```

## NATS JetStream Configuration

```yaml
# Stream configuration
streams:
  ssmd-kalshi:
    subjects:
      - "kalshi.>"
    retention: limits
    max_bytes: 10GB           # Bound total stream size
    max_age: 24h              # Auto-expire old messages
    max_msg_size: 1MB
    discard: old              # Drop oldest when full (not new)
    duplicate_window: 2m      # Dedup window

# Consumer configuration (per client type)
consumers:
  realtime:
    ack_policy: none          # Fire and forget for speed
    max_deliver: 1
    flow_control: true
    idle_heartbeat: 30s

  durable:
    ack_policy: explicit      # Guaranteed delivery
    max_deliver: 5
    ack_wait: 30s
    max_ack_pending: 1000     # Backpressure threshold
```

## Gateway Client Management

Each WebSocket client has a bounded buffer:

```rust
pub struct ClientConnection {
    id: ClientId,
    socket: WebSocketSender,
    buffer: BoundedBuffer,
    subscriptions: HashSet<String>,
    stats: ClientStats,
    state: ClientState,
}

pub struct BoundedBuffer {
    queue: VecDeque<Message>,
    max_size: usize,           // Max messages
    max_bytes: usize,          // Max total bytes
    current_bytes: usize,
    drop_policy: DropPolicy,
}

pub enum DropPolicy {
    DropOldest,                // Drop head of queue
    DropNewest,                // Drop incoming message
    Disconnect,                // Terminate slow client
}

pub enum ClientState {
    Healthy,
    Lagging { since: Instant },
    Dropping { dropped: u64 },
    Disconnecting { reason: String },
}
```

## Conflation for Slow Consumers

Slow consumers can receive conflated snapshots instead of every tick:

```rust
pub enum SubscriptionMode {
    /// Every message, drop if slow
    Realtime,

    /// Periodic snapshots, never drop
    Conflated { interval: Duration },

    /// Latest value only, overwrite on each update
    Latest,
}
```

## Client Subscription API

```json
// Realtime (default)
{"action": "subscribe", "symbols": ["BTCUSD"], "mode": "realtime"}

// Conflated every 100ms
{"action": "subscribe", "symbols": ["BTCUSD"], "mode": "conflated", "interval_ms": 100}

// Latest only (poll-based)
{"action": "subscribe", "symbols": ["BTCUSD"], "mode": "latest"}

// Get current snapshot
{"action": "snapshot", "symbols": ["BTCUSD"]}
```

## Metrics

```prometheus
# Per-client buffer utilization
ssmd_gateway_client_buffer_utilization{client_id="abc123"} 0.45

# Slow consumer count
ssmd_gateway_slow_consumers 2

# Messages dropped due to backpressure
ssmd_gateway_messages_dropped_total{reason="buffer_full"} 1523

# Client lag in milliseconds
ssmd_gateway_client_lag_ms{client_id="abc123"} 250
```

## CLI for Client Management

```bash
# List connected clients
ssmd client list
# ID          STATE    BUFFER  LAG     SUBSCRIPTIONS
# abc123      healthy  12%     50ms    BTCUSD, ETHUSD
# def456      lagging  78%     2500ms  *
# ghi789      dropping 95%     8000ms  BTCUSD

# Get client details
ssmd client show abc123

# Force disconnect a client
ssmd client disconnect def456 --reason "manual intervention"

# Set client to conflated mode
ssmd client set-mode ghi789 --mode conflated --interval 500ms
```
