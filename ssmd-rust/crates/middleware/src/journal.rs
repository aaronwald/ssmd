use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::time::Duration;

use crate::error::JournalError;

/// Journal entry for audit trail
#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub sequence: u64,
    pub timestamp: u64,
    pub topic: String,
    pub key: Option<Bytes>,
    pub payload: Bytes,
    pub headers: HashMap<String, String>,
}

/// Position for reading from journal
#[derive(Debug, Clone)]
pub enum JournalPosition {
    Beginning,
    End,
    Sequence(u64),
    Time(u64),
}

/// Topic configuration
#[derive(Debug, Clone)]
pub struct TopicConfig {
    pub name: String,
    pub retention: Duration,
    pub compaction: bool,
}

/// Journal reader for replay
#[async_trait]
pub trait JournalReader: Send + Sync {
    async fn next(&mut self) -> Result<Option<JournalEntry>, JournalError>;
    async fn seek(&mut self, position: JournalPosition) -> Result<(), JournalError>;
}

/// Journal abstraction for append-only audit log
#[async_trait]
pub trait Journal: Send + Sync {
    async fn append(
        &self,
        topic: &str,
        key: Option<Bytes>,
        payload: Bytes,
    ) -> Result<u64, JournalError>;

    async fn append_with_headers(
        &self,
        topic: &str,
        key: Option<Bytes>,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<u64, JournalError>;

    async fn reader(
        &self,
        topic: &str,
        position: JournalPosition,
    ) -> Result<Box<dyn JournalReader>, JournalError>;

    async fn end_position(&self, topic: &str) -> Result<u64, JournalError>;

    async fn create_topic(&self, config: TopicConfig) -> Result<(), JournalError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_entry_creation() {
        let entry = JournalEntry {
            sequence: 1,
            timestamp: 1703318400000,
            topic: "ssmd.audit".to_string(),
            key: Some(Bytes::from("user:123")),
            payload: Bytes::from(r#"{"action":"login"}"#),
            headers: HashMap::new(),
        };
        assert_eq!(entry.sequence, 1);
        assert_eq!(entry.topic, "ssmd.audit");
    }

    #[test]
    fn test_journal_position_variants() {
        let _begin = JournalPosition::Beginning;
        let _end = JournalPosition::End;
        let _seq = JournalPosition::Sequence(100);
        let _time = JournalPosition::Time(1703318400000);
    }
}
