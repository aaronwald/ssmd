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

        // Get stream and validate filter subject before creating consumer
        let mut stream = jetstream
            .get_stream(&stream_config.stream)
            .await
            .map_err(|e| ArchiverError::Nats(format!("Stream not found: {}", e)))?;

        // Validate filter subject matches stream subjects — crash on mismatch
        // to prevent silent data loss (lifecycle stream incident: 1,856 messages
        // dropped over a month because filter didn't match stream subjects).
        let stream_info = stream
            .info()
            .await
            .map_err(|e| ArchiverError::Nats(format!("Failed to get stream info: {}", e)))?;

        let stream_subjects = &stream_info.config.subjects;
        let filter = &stream_config.filter;

        if !filter_matches_stream_subjects(filter, stream_subjects) {
            return Err(ArchiverError::Nats(format!(
                "Filter subject '{}' does not match any stream '{}' subjects: {:?}. \
                 This will result in zero messages delivered.",
                filter, stream_config.stream, stream_subjects
            )));
        }

        info!(
            stream = %stream_config.stream,
            filter = %filter,
            stream_subjects = ?stream_subjects,
            "Stream subject validation passed"
        );

        let consumer = stream
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

/// Check if a NATS filter subject is compatible with any of the stream's subjects.
/// A filter is compatible if it shares a prefix with a stream subject (before wildcards).
/// e.g., filter "prod.kalshi.json.ticker.>" matches stream "prod.kalshi.>"
fn filter_matches_stream_subjects(filter: &str, stream_subjects: &[String]) -> bool {
    stream_subjects.iter().any(|stream_sub| {
        let stream_prefix = stream_sub
            .trim_end_matches('>')
            .trim_end_matches('*')
            .trim_end_matches('.');
        let filter_prefix = filter
            .trim_end_matches('>')
            .trim_end_matches('*')
            .trim_end_matches('.');
        filter_prefix.starts_with(stream_prefix) || stream_prefix.starts_with(filter_prefix)
    })
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
    use super::{compute_gap_and_next, filter_matches_stream_subjects};

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

    // --- filter_matches_stream_subjects tests ---

    #[test]
    fn test_filter_matches_superset_stream() {
        // Stream "prod.kalshi.>" captures everything; filter is a subset
        let subjects = vec!["prod.kalshi.>".to_string()];
        assert!(filter_matches_stream_subjects("prod.kalshi.json.ticker.>", &subjects));
    }

    #[test]
    fn test_filter_matches_exact_stream_subject() {
        let subjects = vec!["prod.kalshi.json.ticker.>".to_string()];
        assert!(filter_matches_stream_subjects("prod.kalshi.json.ticker.>", &subjects));
    }

    #[test]
    fn test_filter_no_match_different_prefix() {
        // The lifecycle stream incident: stream has lifecycle subjects, filter has json
        let subjects = vec!["prod.kalshi.lifecycle.>".to_string()];
        assert!(!filter_matches_stream_subjects("prod.kalshi.json.lifecycle.>", &subjects));
    }

    #[test]
    fn test_filter_matches_one_of_multiple_subjects() {
        let subjects = vec![
            "prod.kalshi.lifecycle.>".to_string(),
            "prod.kalshi.json.>".to_string(),
        ];
        assert!(filter_matches_stream_subjects("prod.kalshi.json.ticker.>", &subjects));
    }

    #[test]
    fn test_filter_no_match_empty_subjects() {
        let subjects: Vec<String> = vec![];
        assert!(!filter_matches_stream_subjects("prod.kalshi.>", &subjects));
    }

    #[test]
    fn test_filter_matches_with_star_wildcard() {
        let subjects = vec!["prod.kalshi.>".to_string()];
        assert!(filter_matches_stream_subjects("prod.kalshi.json.*", &subjects));
    }

    #[test]
    fn test_filter_no_match_completely_different() {
        let subjects = vec!["prod.kraken.>".to_string()];
        assert!(!filter_matches_stream_subjects("prod.kalshi.json.>", &subjects));
    }

    #[test]
    fn test_filter_matches_stream_is_subset_of_filter() {
        // Stream subject is more specific than filter — still compatible
        let subjects = vec!["prod.kalshi.json.ticker.>".to_string()];
        assert!(filter_matches_stream_subjects("prod.kalshi.>", &subjects));
    }
}
