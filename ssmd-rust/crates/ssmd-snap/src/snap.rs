use async_nats::jetstream::{self, consumer::pull::Config as ConsumerConfig, Context};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;

use crate::metrics::Metrics;

/// Configuration for a single stream subscription.
pub struct StreamConfig {
    pub stream_name: String,
    pub feed: String,
    pub filter_subject: String,
}

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

        // Parse payload, extract ticker, inject _snap_at in one pass
        let payload = &msg.payload;
        let (ticker, enriched) = match extract_and_enrich(payload) {
            Some(v) => v,
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
            .set(&redis_key, &enriched)
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

/// Extract ticker and inject `_snap_at` timestamp in a single JSON parse.
/// Returns (ticker, enriched_payload_bytes) or None if no ticker found.
fn extract_and_enrich(payload: &[u8]) -> Option<(String, Vec<u8>)> {
    let mut v: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let obj = v.as_object_mut()?;

    // Try known identifier fields across exchanges:
    //   Kalshi:     market_ticker
    //   Kraken:     product_id
    //   Polymarket: market (condition_id hex)
    let keys = &["market_ticker", "product_id", "market"];

    let mut ticker: Option<String> = None;

    // Check top-level fields
    for key in keys {
        if let Some(val) = obj.get(*key).and_then(|v| v.as_str()) {
            ticker = Some(val.to_string());
            break;
        }
    }

    // Check inside "msg" wrapper (Kalshi connector wraps ticker data)
    if ticker.is_none() {
        if let Some(msg) = obj.get("msg").and_then(|v| v.as_object()) {
            for key in keys {
                if let Some(val) = msg.get(*key).and_then(|v| v.as_str()) {
                    ticker = Some(val.to_string());
                    break;
                }
            }
        }
    }

    let ticker = ticker?;

    // Inject _snap_at timestamp (epoch millis) for staleness detection
    let now_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    obj.insert("_snap_at".to_string(), serde_json::json!(now_millis));

    let enriched = serde_json::to_vec(&v).ok()?;
    Some((ticker, enriched))
}

/// Extract ticker only (used by tests, no enrichment).
#[cfg(test)]
fn extract_ticker(payload: &[u8]) -> Option<String> {
    extract_and_enrich(payload).map(|(t, _)| t)
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
        let payload = br#"{"product_id":"PF_XBTUSD","bid":63990.0,"ask":63991.0}"#;
        assert_eq!(extract_ticker(payload), Some("PF_XBTUSD".into()));
    }

    #[test]
    fn test_extract_polymarket_ticker() {
        let payload = br#"{"market":"0x713e73c0e77492732924655dea2ad9ac12f47c0635ae013712b3da250583992e","event_type":"price_change"}"#;
        assert_eq!(extract_ticker(payload), Some("0x713e73c0e77492732924655dea2ad9ac12f47c0635ae013712b3da250583992e".into()));
    }

    #[test]
    fn test_extract_kalshi_nested_msg() {
        let payload = br#"{"type":"ticker","sid":1,"msg":{"market_ticker":"KXBTCD-26FEB21-T100250","yes_bid":50}}"#;
        assert_eq!(extract_ticker(payload), Some("KXBTCD-26FEB21-T100250".into()));
    }

    #[test]
    fn test_extract_missing_ticker() {
        let payload = br#"{"volume":100}"#;
        assert_eq!(extract_ticker(payload), None);
    }

    #[test]
    fn test_enrich_injects_snap_at() {
        let payload = br#"{"market_ticker":"KXBTCD-26FEB21-T100250","yes_bid":50}"#;
        let (ticker, enriched) = extract_and_enrich(payload).unwrap();
        assert_eq!(ticker, "KXBTCD-26FEB21-T100250");
        let v: serde_json::Value = serde_json::from_slice(&enriched).unwrap();
        assert!(v.get("_snap_at").unwrap().is_u64());
        assert_eq!(v.get("market_ticker").unwrap().as_str().unwrap(), "KXBTCD-26FEB21-T100250");
        assert_eq!(v.get("yes_bid").unwrap().as_i64().unwrap(), 50);
    }

    #[test]
    fn test_enrich_invalid_json() {
        assert!(extract_and_enrich(b"not json").is_none());
    }
}
