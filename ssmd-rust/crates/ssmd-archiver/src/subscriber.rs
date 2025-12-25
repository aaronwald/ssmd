use async_nats::jetstream::{self, consumer::PullConsumer};
use futures_util::StreamExt;
use tracing::{error, info, trace, warn};

use crate::config::NatsConfig;
use crate::error::ArchiverError;

pub struct Subscriber {
    consumer: PullConsumer,
    expected_seq: Option<u64>,
}

pub struct ReceivedMessage {
    pub data: Vec<u8>,
    pub seq: u64,
}

impl Subscriber {
    pub async fn connect(config: &NatsConfig) -> Result<Self, ArchiverError> {
        let client = async_nats::connect(&config.url)
            .await
            .map_err(|e| ArchiverError::Nats(e.to_string()))?;

        let jetstream = jetstream::new(client);

        // Get or create consumer
        let consumer = jetstream
            .get_stream(&config.stream)
            .await
            .map_err(|e| ArchiverError::Nats(format!("Stream not found: {}", e)))?
            .get_or_create_consumer(
                &config.consumer,
                jetstream::consumer::pull::Config {
                    durable_name: Some(config.consumer.clone()),
                    filter_subject: config.filter.clone(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ArchiverError::Nats(e.to_string()))?;

        info!(
            stream = %config.stream,
            consumer = %config.consumer,
            filter = %config.filter,
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
                    });

                    // Ack the message
                    if let Err(e) = msg.ack().await {
                        error!(error = %e, seq = seq, "Failed to ack message");
                    }
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
