use async_nats::jetstream::{self, consumer::PullConsumer, message::Message};
use futures_util::StreamExt;
use tracing::{error, info, trace, warn};

use crate::config::StreamConfig;
use crate::error::ArchiverError;

pub struct Subscriber {
    consumer: PullConsumer,
    expected_seq: Option<u64>,
}

/// A received message that must be explicitly acked after processing
pub struct ReceivedMessage {
    pub data: Vec<u8>,
    pub seq: u64,
    message: Message,
}

impl ReceivedMessage {
    /// Acknowledge the message after successful processing
    pub async fn ack(self) -> Result<(), ArchiverError> {
        self.message
            .ack()
            .await
            .map_err(|e| ArchiverError::Nats(format!("Failed to ack: {}", e)))
    }
}

impl Subscriber {
    /// Connect to NATS and create a subscriber for a specific stream
    pub async fn connect(nats_url: &str, stream_config: &StreamConfig) -> Result<Self, ArchiverError> {
        let client = async_nats::connect(nats_url)
            .await
            .map_err(|e| ArchiverError::Nats(e.to_string()))?;

        let jetstream = jetstream::new(client);

        // Get or create consumer
        let consumer = jetstream
            .get_stream(&stream_config.stream)
            .await
            .map_err(|e| ArchiverError::Nats(format!("Stream not found: {}", e)))?
            .get_or_create_consumer(
                &stream_config.consumer,
                jetstream::consumer::pull::Config {
                    durable_name: Some(stream_config.consumer.clone()),
                    filter_subject: stream_config.filter.clone(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ArchiverError::Nats(e.to_string()))?;

        info!(
            stream = %stream_config.stream,
            consumer = %stream_config.consumer,
            filter = %stream_config.filter,
            "Connected to NATS JetStream"
        );

        Ok(Self {
            consumer,
            expected_seq: None,
        })
    }

    /// Fetch next batch of messages
    pub async fn fetch(&mut self, batch_size: usize) -> Result<Vec<ReceivedMessage>, ArchiverError> {
        let messages = self
            .consumer
            .fetch()
            .max_messages(batch_size)
            .messages()
            .await
            .map_err(|e| ArchiverError::Nats(e.to_string()))?;

        let mut result = Vec::new();

        tokio::pin!(messages);
        while let Some(msg_result) = messages.next().await {
            match msg_result {
                Ok(msg) => {
                    let seq = msg.info().map(|i| i.stream_sequence).unwrap_or(0);

                    // Check for gaps
                    if let Some(expected) = self.expected_seq {
                        if seq > expected {
                            warn!(
                                expected = expected,
                                actual = seq,
                                gap = seq - expected,
                                "Gap detected in sequence"
                            );
                        }
                    }
                    self.expected_seq = Some(seq + 1);

                    result.push(ReceivedMessage {
                        data: msg.payload.to_vec(),
                        seq,
                        message: msg,
                    });
                    // Note: ack deferred until after successful write
                }
                Err(e) => {
                    error!(error = %e, "Error receiving message");
                }
            }
        }

        trace!(count = result.len(), "Fetched messages");
        Ok(result)
    }

    /// Check if there was a gap (returns gap info for manifest)
    pub fn check_gap(&self, seq: u64) -> Option<(u64, u64)> {
        if let Some(expected) = self.expected_seq {
            if seq > expected {
                return Some((expected - 1, seq - expected));
            }
        }
        None
    }
}
