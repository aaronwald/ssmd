use async_nats::jetstream::{self, consumer::pull::Config as ConsumerConfig, Context};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;

use crate::config::StreamConfig;
use crate::metrics::Metrics;

/// Run the snap loop for a single stream: subscribe to NATS ticker subjects
/// and write each message to Redis with a TTL.
pub async fn run_snap(
    js: Context,
    redis_conn: redis::aio::MultiplexedConnection,
    stream_config: StreamConfig,
    ttl_secs: u64,
    metrics: Arc<Metrics>,
) {
    let feed = &stream_config.feed;
    tracing::info!(
        stream = %stream_config.stream_name,
        feed,
        filter = %stream_config.filter_subject,
        ttl_secs,
        "starting snap consumer"
    );

    loop {
        match run_snap_inner(&js, &redis_conn, &stream_config, ttl_secs, &metrics).await {
            Ok(()) => {
                tracing::info!(feed, "snap consumer stream ended, restarting");
            }
            Err(e) => {
                tracing::error!(feed, error = %e, "snap consumer error, restarting in 5s");
                metrics.errors.with_label_values(&[feed, "consumer"]).inc();
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn run_snap_inner(
    js: &Context,
    redis_conn: &redis::aio::MultiplexedConnection,
    stream_config: &StreamConfig,
    ttl_secs: u64,
    metrics: &Metrics,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let feed = &stream_config.feed;

    let stream = js
        .get_stream(&stream_config.stream_name)
        .await
        .map_err(|e| format!("get stream {}: {}", stream_config.stream_name, e))?;

    // Ephemeral pull consumer — no durable name, deliver latest per subject
    let consumer = stream
        .create_consumer(ConsumerConfig {
            filter_subject: stream_config.filter_subject.clone(),
            deliver_policy: jetstream::consumer::DeliverPolicy::LastPerSubject,
            ack_policy: jetstream::consumer::AckPolicy::None,
            ..Default::default()
        })
        .await
        .map_err(|e| format!("create consumer for {}: {}", stream_config.stream_name, e))?;

    let mut messages = consumer
        .messages()
        .await
        .map_err(|e| format!("get messages for {}: {}", stream_config.stream_name, e))?;

    tracing::info!(feed, "consumer connected, processing messages");

    while let Some(msg_result) = messages.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(feed, error = %e, "message receive error");
                metrics.errors.with_label_values(&[feed, "receive"]).inc();
                continue;
            }
        };

        metrics.messages_received.with_label_values(&[feed]).inc();

        // Parse the ticker field to build the Redis key
        let payload = &msg.payload;
        let ticker = match extract_ticker(payload) {
            Some(t) => t,
            None => {
                tracing::debug!(feed, "message missing ticker field, skipping");
                metrics.errors.with_label_values(&[feed, "parse"]).inc();
                continue;
            }
        };

        let redis_key = format!("snap:{}:{}", feed, ticker);

        // Pipeline SET + EXPIRE in one round trip
        let mut conn = redis_conn.clone();
        let result: Result<(), redis::RedisError> = redis::pipe()
            .set(&redis_key, payload.as_ref())
            .expire(&redis_key, ttl_secs as i64)
            .query_async(&mut conn)
            .await;

        match result {
            Ok(()) => {
                metrics.redis_writes.with_label_values(&[feed]).inc();
            }
            Err(e) => {
                tracing::warn!(feed, key = %redis_key, error = %e, "Redis write failed");
                metrics.errors.with_label_values(&[feed, "redis"]).inc();
            }
        }
    }

    Ok(())
}

/// Extract the market_ticker (Kalshi), pair_id (Kraken), or token_id (Polymarket)
/// from a JSON payload. Tries known field names in order.
fn extract_ticker(payload: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let obj = v.as_object()?;

    // Try in order: market_ticker (Kalshi), pair_id (Kraken), token_id (Polymarket)
    for key in &["market_ticker", "pair_id", "token_id"] {
        if let Some(val) = obj.get(*key).and_then(|v| v.as_str()) {
            return Some(val.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_kalshi_ticker() {
        let payload = br#"{"market_ticker":"KXBTCD-26FEB21-T100250","yes_bid":50}"#;
        assert_eq!(extract_ticker(payload), Some("KXBTCD-26FEB21-T100250".into()));
    }

    #[test]
    fn test_extract_kraken_ticker() {
        let payload = br#"{"pair_id":"PI_XBTUSD","bid":45000}"#;
        assert_eq!(extract_ticker(payload), Some("PI_XBTUSD".into()));
    }

    #[test]
    fn test_extract_polymarket_ticker() {
        let payload = br#"{"token_id":"abc123","best_bid":0.55}"#;
        assert_eq!(extract_ticker(payload), Some("abc123".into()));
    }

    #[test]
    fn test_extract_missing_ticker() {
        let payload = br#"{"volume":100}"#;
        assert_eq!(extract_ticker(payload), None);
    }
}
