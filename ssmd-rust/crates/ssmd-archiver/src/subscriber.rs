use async_nats::jetstream::{self, consumer::PullConsumer, message::Message};
use futures_util::StreamExt;
use std::time::Duration;
use tracing::{error, info, trace, warn};

use crate::config::StreamConfig;
use crate::error::ArchiverError;

pub struct Subscriber {
    consumer: PullConsumer,
    expected_seq: Option<u64>,
}

/// A received message that must be explicitly acked after processing
pub struct ReceivedMessage {
    pub seq: u64,
    pub gap: Option<(u64, u64)>,
    message: Message,
}

impl ReceivedMessage {
    /// Access message payload without copying.
    pub fn payload(&self) -> &[u8] {
        &self.message.payload
    }

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
            .heartbeat(Duration::from_secs(5)) // Heartbeat to detect stale connections (must be < expires)
            .expires(Duration::from_secs(30)) // Timeout to prevent indefinite hangs
            .messages()
            .await
            .map_err(|e| ArchiverError::Nats(e.to_string()))?;

        let mut result = Vec::new();

        tokio::pin!(messages);
        while let Some(msg_result) = messages.next().await {
            match msg_result {
                Ok(msg) => {
                    let seq = msg.info().map(|i| i.stream_sequence).unwrap_or(0);
                    let (gap, next_expected) = compute_gap_and_next(self.expected_seq, seq);

                    if let Some((after_seq, missing_count)) = gap {
                        warn!(
                            expected = self.expected_seq.unwrap_or_default(),
                            actual = seq,
                            gap = missing_count,
                            after_seq = after_seq,
                            "Gap detected in sequence"
                        );
                    }

                    self.expected_seq = next_expected;

                    result.push(ReceivedMessage {
                        seq,
                        gap,
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

}

fn compute_gap_and_next(expected_seq: Option<u64>, seq: u64) -> (Option<(u64, u64)>, Option<u64>) {
    let gap = match expected_seq {
        Some(expected) if seq > expected => Some((expected.saturating_sub(1), seq - expected)),
        _ => None,
    };

    // Never regress expected_seq on out-of-order/redelivered messages.
    // seq == 0 is a sentinel (no info available) — preserve current expectation.
    let next_expected = match (expected_seq, seq) {
        (_, 0) => expected_seq,
        (Some(exp), _) if seq < exp => expected_seq,
        _ => Some(seq + 1),
    };

    (gap, next_expected)
}

#[cfg(test)]
mod tests {
    use super::compute_gap_and_next;

    #[test]
    fn test_compute_gap_and_next_detects_gap() {
        let (gap, next) = compute_gap_and_next(Some(101), 105);
        assert_eq!(gap, Some((100, 4)));
        assert_eq!(next, Some(106));
    }

    #[test]
    fn test_compute_gap_and_next_no_gap_in_order() {
        let (gap, next) = compute_gap_and_next(Some(101), 101);
        assert_eq!(gap, None);
        assert_eq!(next, Some(102));
    }

    #[test]
    fn test_compute_gap_and_next_ignores_zero_seq_for_next() {
        let (gap, next) = compute_gap_and_next(Some(17), 0);
        assert_eq!(gap, None);
        assert_eq!(next, Some(17));
    }

    #[test]
    fn test_compute_gap_and_next_no_regress_on_redelivery() {
        // After seeing seq 100, expected is 101. Redelivered seq 50 must not regress.
        let (gap, next) = compute_gap_and_next(Some(101), 50);
        assert_eq!(gap, None);
        assert_eq!(next, Some(101)); // stays at 101, not 51
    }

    #[test]
    fn test_compute_gap_and_next_no_regress_on_duplicate() {
        // Exact duplicate of last seen message
        let (gap, next) = compute_gap_and_next(Some(101), 100);
        assert_eq!(gap, None);
        assert_eq!(next, Some(101)); // stays at 101
    }

    #[test]
    fn test_compute_gap_and_next_first_message() {
        let (gap, next) = compute_gap_and_next(None, 42);
        assert_eq!(gap, None);
        assert_eq!(next, Some(43));
    }
}
