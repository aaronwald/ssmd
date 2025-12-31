//! NATS JetStream publisher for CDC events

use async_nats::jetstream::{self, Context};
use crate::{Error, Result, messages::CdcEvent};

pub struct Publisher {
    js: Context,
    stream_name: String,
}

impl Publisher {
    pub async fn new(nats_url: &str, stream_name: &str) -> Result<Self> {
        let client = async_nats::connect(nats_url).await
            .map_err(|e| Error::Config(format!("NATS connection failed: {}", e)))?;
        let js = jetstream::new(client);

        Ok(Self {
            js,
            stream_name: stream_name.to_string(),
        })
    }

    /// Ensure the CDC stream exists
    pub async fn ensure_stream(&self) -> Result<()> {
        let config = jetstream::stream::Config {
            name: self.stream_name.clone(),
            subjects: vec!["cdc.>".into()],
            max_messages: 100_000,
            max_age: std::time::Duration::from_secs(7 * 24 * 60 * 60), // 7 days
            storage: jetstream::stream::StorageType::File,
            ..Default::default()
        };

        match self.js.get_stream(&self.stream_name).await {
            Ok(_) => {
                tracing::info!(stream = %self.stream_name, "Stream already exists");
            }
            Err(_) => {
                self.js.create_stream(config).await
                    .map_err(|e| Error::Config(format!("Failed to create stream: {}", e)))?;
                tracing::info!(stream = %self.stream_name, "Created stream");
            }
        }

        Ok(())
    }

    /// Publish a CDC event
    pub async fn publish(&self, event: &CdcEvent) -> Result<()> {
        let subject = format!("cdc.{}.{}", event.table, event.op.as_str());
        let payload = serde_json::to_vec(event)?;

        self.js.publish(subject.clone(), payload.into()).await
            .map_err(|e| Error::Config(format!("Publish failed: {}", e)))?
            .await
            .map_err(|e| Error::Config(format!("Publish ack failed: {}", e)))?;

        tracing::debug!(subject = %subject, table = %event.table, "Published CDC event");
        Ok(())
    }
}
