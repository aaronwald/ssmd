use async_nats::jetstream::{self, consumer::pull::Stream, Context};
use futures_util::StreamExt;
use std::time::Duration;
use crate::{Result, Error, cache::RedisCache};

/// CDC event from NATS (matches ssmd-cdc publisher format)
#[derive(Debug, serde::Deserialize)]
pub struct CdcEvent {
    pub lsn: String,
    pub table: String,
    pub op: String,  // "insert", "update", "delete"
    pub key: serde_json::Value,
    pub data: Option<serde_json::Value>,
}

pub struct CdcConsumer {
    stream: Stream,
    snapshot_lsn: String,
}

impl CdcConsumer {
    pub async fn new(
        nats_url: &str,
        stream_name: &str,
        consumer_name: &str,
        snapshot_lsn: String,
    ) -> Result<Self> {
        let client = async_nats::connect(nats_url).await
            .map_err(|e| Error::Nats(format!("Connection failed: {}", e)))?;
        let js: Context = jetstream::new(client);

        // Get or create consumer
        let stream_obj = js.get_stream(stream_name).await
            .map_err(|e| Error::Nats(format!("Get stream failed: {}", e)))?;

        let consumer = stream_obj
            .get_or_create_consumer(
                consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(consumer_name.to_string()),
                    filter_subject: "cdc.>".to_string(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| Error::Nats(format!("Create consumer failed: {}", e)))?;

        // Set heartbeat to 5s to detect stale connections
        let messages = consumer.stream()
            .heartbeat(Duration::from_secs(5))
            .messages()
            .await
            .map_err(|e| Error::Nats(format!("Get messages failed: {}", e)))?;

        Ok(Self {
            stream: messages,
            snapshot_lsn,
        })
    }

    /// Compare LSNs (format: "0/16B3748")
    fn lsn_gte(&self, lsn: &str, threshold: &str) -> bool {
        // Simple string comparison works for LSN format
        lsn >= threshold
    }

    /// Process CDC events and update cache
    pub async fn run(&mut self, cache: &RedisCache) -> Result<()> {
        tracing::info!(snapshot_lsn = %self.snapshot_lsn, "Starting CDC consumer");

        let mut processed: u64 = 0;
        let mut skipped: u64 = 0;

        while let Some(msg) = self.stream.next().await {
            let msg = msg.map_err(|e| Error::Nats(format!("Message error: {}", e)))?;

            match serde_json::from_slice::<CdcEvent>(&msg.payload) {
                Ok(event) => {
                    // Skip events before snapshot LSN
                    if !self.lsn_gte(&event.lsn, &self.snapshot_lsn) {
                        skipped += 1;
                        msg.ack().await.map_err(|e| Error::Nats(format!("Ack failed: {}", e)))?;
                        continue;
                    }

                    // Extract key (assumes first field is the key)
                    let key = match &event.key {
                        serde_json::Value::Object(obj) => {
                            obj.values().next()
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        }
                        _ => None,
                    };

                    if let Some(key) = key {
                        match event.op.as_str() {
                            "insert" | "update" => {
                                if let Some(data) = &event.data {
                                    cache.set(&event.table, &key, data).await?;
                                }
                            }
                            "delete" => {
                                cache.delete(&event.table, &key).await?;
                            }
                            _ => {}
                        }
                    }

                    processed += 1;
                    if processed % 100 == 0 {
                        tracing::info!(processed, skipped, "CDC events processed");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to parse CDC event");
                }
            }

            msg.ack().await.map_err(|e| Error::Nats(format!("Ack failed: {}", e)))?;
        }

        Ok(())
    }
}
