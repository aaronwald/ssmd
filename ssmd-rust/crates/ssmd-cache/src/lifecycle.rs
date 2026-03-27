use async_nats::jetstream::{self, consumer::pull::Stream, Context};
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use futures_util::StreamExt;
use std::time::Duration;

use crate::{Error, Result, metrics::CacheMetrics};

const MAX_CONSECUTIVE_ERRORS: u32 = 5;

/// Terminal lifecycle event types that trigger market status updates
const TERMINAL_EVENTS: &[&str] = &["determined", "settled", "closed", "finalized", "deactivated"];

fn is_terminal_event(event_type: &str) -> bool {
    TERMINAL_EVENTS.contains(&event_type)
}

fn epoch_to_datetime(epoch: Option<i64>) -> Option<DateTime<Utc>> {
    epoch.and_then(|ts| DateTime::from_timestamp(ts, 0))
}

fn build_metadata(msg: &LifecycleMsg) -> serde_json::Value {
    let mut meta = match &msg.additional_metadata {
        Some(v) if v.is_object() => v.clone(),
        _ => serde_json::json!({}),
    };
    if let Some(result) = &msg.result {
        if let Some(obj) = meta.as_object_mut() {
            obj.insert("result".to_string(), serde_json::Value::String(result.clone()));
        }
    }
    meta
}

#[derive(Debug, serde::Deserialize)]
pub struct RawLifecycleMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub msg: LifecycleMsg,
}

#[derive(Debug, serde::Deserialize)]
pub struct LifecycleMsg {
    pub market_ticker: String,
    pub event_type: String,
    pub open_ts: Option<i64>,
    pub close_ts: Option<i64>,
    pub determination_ts: Option<i64>,
    pub settled_ts: Option<i64>,
    pub result: Option<String>,
    pub additional_metadata: Option<serde_json::Value>,
}

pub struct LifecycleConsumer {
    stream: Stream,
    pool: Pool,
    metrics: CacheMetrics,
}

impl LifecycleConsumer {
    pub async fn new(
        nats_url: &str,
        stream_name: &str,
        consumer_name: &str,
        filter_subject: &str,
        pool: Pool,
        metrics: CacheMetrics,
    ) -> Result<Self> {
        let client = async_nats::connect(nats_url).await
            .map_err(|e| Error::Nats(format!("Lifecycle NATS connect failed: {e}")))?;
        let js: Context = jetstream::new(client);

        let stream_obj = js.get_stream(stream_name).await
            .map_err(|e| Error::Nats(format!("Get lifecycle stream failed: {e}")))?;

        let consumer = stream_obj
            .get_or_create_consumer(
                consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(consumer_name.to_string()),
                    filter_subject: filter_subject.to_string(),
                    deliver_policy: jetstream::consumer::DeliverPolicy::Last,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| Error::Nats(format!("Create lifecycle consumer failed: {e}")))?;

        let messages = consumer.stream()
            .heartbeat(Duration::from_secs(5))
            .messages()
            .await
            .map_err(|e| Error::Nats(format!("Lifecycle messages stream failed: {e}")))?;

        Ok(Self { stream: messages, pool, metrics })
    }

    pub async fn run(&mut self) -> Result<()> {
        tracing::info!("Starting lifecycle consumer");

        let mut processed: u64 = 0;
        let mut consecutive_errors: u32 = 0;

        while let Some(msg) = self.stream.next().await {
            let msg = msg.map_err(|e| Error::Nats(format!("Lifecycle message error: {e}")))?;

            let nats_seq = msg.info()
                .map(|i| i.stream_sequence)
                .unwrap_or(0);

            match self.process_message(&msg.payload, nats_seq).await {
                Ok(true) => {
                    processed += 1;
                    consecutive_errors = 0;
                    if processed <= 5 || processed % 100 == 0 {
                        tracing::info!(processed, "Lifecycle events processed");
                    }
                }
                Ok(false) => {
                    // Skipped (not lifecycle_v2, missing fields, etc.)
                }
                Err(e) => {
                    consecutive_errors += 1;
                    self.metrics.lifecycle_errors.inc();
                    tracing::error!(
                        error = %e,
                        consecutive_errors,
                        "Lifecycle processing error"
                    );

                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        tracing::error!("{MAX_CONSECUTIVE_ERRORS} consecutive lifecycle errors — crashing");
                        return Err(e);
                    }

                    msg.ack().await.map_err(|e| Error::Nats(format!("ACK after error failed: {e}")))?;
                    continue;
                }
            }

            msg.ack().await.map_err(|e| Error::Nats(format!("Lifecycle ACK failed: {e}")))?;
        }

        Err(Error::Nats("Lifecycle message stream ended unexpectedly".into()))
    }

    async fn process_message(&self, payload: &[u8], nats_seq: u64) -> Result<bool> {
        let raw: RawLifecycleMessage = match serde_json::from_slice(payload) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse lifecycle message");
                return Ok(false);
            }
        };

        if raw.msg_type != "market_lifecycle_v2" {
            return Ok(false);
        }

        let m = &raw.msg;
        if m.market_ticker.is_empty() || m.event_type.is_empty() {
            tracing::warn!("Skipping lifecycle message with missing fields");
            return Ok(false);
        }

        self.metrics.lifecycle_events.with_label_values(&[&m.event_type]).inc();

        let mut client = self.pool.get().await?;
        let tx = client.transaction().await
            .map_err(|e| Error::Database(format!("Transaction start failed: {e}")))?;

        // INSERT lifecycle event — idempotent via nats_seq
        let metadata = build_metadata(m);
        let settled_ts = epoch_to_datetime(m.settled_ts.or(m.determination_ts));
        let nats_seq_i64 = nats_seq as i64;

        tx.execute(
            "INSERT INTO market_lifecycle_events (market_ticker, event_type, open_ts, close_ts, settled_ts, metadata, nats_seq)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (nats_seq) WHERE nats_seq IS NOT NULL DO NOTHING",
            &[
                &m.market_ticker,
                &m.event_type,
                &epoch_to_datetime(m.open_ts),
                &epoch_to_datetime(m.close_ts),
                &settled_ts,
                &metadata,
                &nats_seq_i64,
            ],
        ).await.map_err(|e| Error::Database(format!("Lifecycle INSERT failed: {e}")))?;

        self.metrics.lifecycle_db_writes.inc();

        // UPDATE market status for terminal events
        if is_terminal_event(&m.event_type) {
            let result = tx.execute(
                "UPDATE markets SET status = $1, updated_at = NOW() WHERE ticker = $2",
                &[&m.event_type, &m.market_ticker],
            ).await.map_err(|e| Error::Database(format!("Market status UPDATE failed: {e}")))?;

            if result > 0 {
                tracing::info!(
                    ticker = %m.market_ticker,
                    status = %m.event_type,
                    "Market status updated"
                );
                self.metrics.lifecycle_status_updates.inc();
            }
        } else if m.event_type == "close_date_updated" {
            if let Some(close_dt) = epoch_to_datetime(m.close_ts) {
                tx.execute(
                    "UPDATE markets SET close_time = $1, updated_at = NOW() WHERE ticker = $2",
                    &[&close_dt, &m.market_ticker],
                ).await.map_err(|e| Error::Database(format!("Close time UPDATE failed: {e}")))?;
            }
        }

        tx.commit().await
            .map_err(|e| Error::Database(format!("Transaction commit failed: {e}")))?;

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lifecycle_message_valid() {
        let json = r#"{"type":"market_lifecycle_v2","msg":{"market_ticker":"KXBTCD-26MAR0211-T5060","event_type":"settled","settled_ts":1711476000}}"#;
        let msg: RawLifecycleMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "market_lifecycle_v2");
        assert_eq!(msg.msg.market_ticker, "KXBTCD-26MAR0211-T5060");
        assert_eq!(msg.msg.event_type, "settled");
        assert_eq!(msg.msg.settled_ts, Some(1711476000));
    }

    #[test]
    fn test_parse_lifecycle_message_missing_optional_fields() {
        let json = r#"{"type":"market_lifecycle_v2","msg":{"market_ticker":"KXBTCD-26MAR0211-T5060","event_type":"activated"}}"#;
        let msg: RawLifecycleMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg.event_type, "activated");
        assert!(msg.msg.open_ts.is_none());
        assert!(msg.msg.close_ts.is_none());
        assert!(msg.msg.settled_ts.is_none());
        assert!(msg.msg.result.is_none());
        assert!(msg.msg.additional_metadata.is_none());
    }

    #[test]
    fn test_parse_non_lifecycle_message() {
        let json = r#"{"type":"ticker","msg":{"market_ticker":"X","event_type":"y"}}"#;
        let msg: RawLifecycleMessage = serde_json::from_str(json).unwrap();
        assert_ne!(msg.msg_type, "market_lifecycle_v2");
    }

    #[test]
    fn test_parse_malformed_message() {
        let json = r#"{"garbage": true}"#;
        assert!(serde_json::from_str::<RawLifecycleMessage>(json).is_err());
    }

    #[test]
    fn test_is_terminal_event() {
        assert!(is_terminal_event("determined"));
        assert!(is_terminal_event("settled"));
        assert!(is_terminal_event("closed"));
        assert!(is_terminal_event("finalized"));
        assert!(is_terminal_event("deactivated"));
        assert!(!is_terminal_event("created"));
        assert!(!is_terminal_event("activated"));
        assert!(!is_terminal_event("close_date_updated"));
        assert!(!is_terminal_event("unknown_type"));
    }

    #[test]
    fn test_epoch_to_datetime() {
        let dt = epoch_to_datetime(Some(1711476000));
        assert!(dt.is_some());

        assert!(epoch_to_datetime(None).is_none());
        assert!(epoch_to_datetime(Some(0)).is_some());
    }

    #[test]
    fn test_build_metadata_with_result_and_extras() {
        let msg = LifecycleMsg {
            market_ticker: "TEST".into(),
            event_type: "settled".into(),
            open_ts: None,
            close_ts: None,
            determination_ts: None,
            settled_ts: None,
            result: Some("yes".into()),
            additional_metadata: Some(serde_json::json!({"foo": "bar"})),
        };
        let meta = build_metadata(&msg);
        assert_eq!(meta["result"], "yes");
        assert_eq!(meta["foo"], "bar");
    }

    #[test]
    fn test_build_metadata_no_extras() {
        let msg = LifecycleMsg {
            market_ticker: "TEST".into(),
            event_type: "activated".into(),
            open_ts: None,
            close_ts: None,
            determination_ts: None,
            settled_ts: None,
            result: None,
            additional_metadata: None,
        };
        let meta = build_metadata(&msg);
        assert!(meta.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_build_metadata_result_only() {
        let msg = LifecycleMsg {
            market_ticker: "TEST".into(),
            event_type: "determined".into(),
            open_ts: None,
            close_ts: None,
            determination_ts: None,
            settled_ts: None,
            result: Some("no".into()),
            additional_metadata: None,
        };
        let meta = build_metadata(&msg);
        assert_eq!(meta["result"], "no");
        assert_eq!(meta.as_object().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_full_lifecycle_message() {
        let json = r#"{
            "type": "market_lifecycle_v2",
            "sid": 42,
            "msg": {
                "market_ticker": "KXNBAGAME-26MAR15DETTOR-TOR",
                "event_type": "determined",
                "open_ts": 1711000000,
                "close_ts": 1711100000,
                "determination_ts": 1711200000,
                "settled_ts": 1711300000,
                "result": "yes",
                "additional_metadata": {"payout": 1.0}
            }
        }"#;
        let msg: RawLifecycleMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg.market_ticker, "KXNBAGAME-26MAR15DETTOR-TOR");
        assert_eq!(msg.msg.open_ts, Some(1711000000));
        assert_eq!(msg.msg.determination_ts, Some(1711200000));
        assert_eq!(msg.msg.result, Some("yes".into()));
    }
}
