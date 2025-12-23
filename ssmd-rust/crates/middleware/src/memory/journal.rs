use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::error::JournalError;
use crate::journal::{Journal, JournalEntry, JournalPosition, JournalReader, TopicConfig};

pub struct InMemoryJournal {
    topics: Arc<RwLock<HashMap<String, Vec<JournalEntry>>>>,
    sequence: Arc<Mutex<u64>>,
}

impl InMemoryJournal {
    pub fn new() -> Self {
        Self { topics: Arc::new(RwLock::new(HashMap::new())), sequence: Arc::new(Mutex::new(0)) }
    }

    async fn next_sequence(&self) -> u64 {
        let mut seq = self.sequence.lock().await;
        *seq += 1;
        *seq
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
    }
}

impl Default for InMemoryJournal {
    fn default() -> Self { Self::new() }
}

struct InMemoryJournalReader {
    entries: Vec<JournalEntry>,
    position: usize,
}

#[async_trait]
impl JournalReader for InMemoryJournalReader {
    async fn next(&mut self) -> Result<Option<JournalEntry>, JournalError> {
        if self.position >= self.entries.len() { Ok(None) }
        else { let entry = self.entries[self.position].clone(); self.position += 1; Ok(Some(entry)) }
    }

    async fn seek(&mut self, position: JournalPosition) -> Result<(), JournalError> {
        match position {
            JournalPosition::Beginning => self.position = 0,
            JournalPosition::End => self.position = self.entries.len(),
            JournalPosition::Sequence(seq) => self.position = self.entries.iter().position(|e| e.sequence >= seq).unwrap_or(self.entries.len()),
            JournalPosition::Time(ts) => self.position = self.entries.iter().position(|e| e.timestamp >= ts).unwrap_or(self.entries.len()),
        }
        Ok(())
    }
}

#[async_trait]
impl Journal for InMemoryJournal {
    async fn append(&self, topic: &str, key: Option<Bytes>, payload: Bytes) -> Result<u64, JournalError> {
        self.append_with_headers(topic, key, payload, HashMap::new()).await
    }

    async fn append_with_headers(&self, topic: &str, key: Option<Bytes>, payload: Bytes, headers: HashMap<String, String>) -> Result<u64, JournalError> {
        let seq = self.next_sequence().await;
        let entry = JournalEntry { sequence: seq, timestamp: Self::now_millis(), topic: topic.to_string(), key, payload, headers };
        let mut topics = self.topics.write().await;
        topics.entry(topic.to_string()).or_default().push(entry);
        Ok(seq)
    }

    async fn reader(&self, topic: &str, position: JournalPosition) -> Result<Box<dyn JournalReader>, JournalError> {
        let topics = self.topics.read().await;
        let entries = topics.get(topic).cloned().unwrap_or_default();
        let mut reader = InMemoryJournalReader { entries, position: 0 };
        reader.seek(position).await?;
        Ok(Box::new(reader))
    }

    async fn end_position(&self, topic: &str) -> Result<u64, JournalError> {
        let topics = self.topics.read().await;
        Ok(topics.get(topic).and_then(|entries| entries.last().map(|e| e.sequence)).unwrap_or(0))
    }

    async fn create_topic(&self, config: TopicConfig) -> Result<(), JournalError> {
        let mut topics = self.topics.write().await;
        topics.entry(config.name).or_default();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_append_and_read() {
        let journal = InMemoryJournal::new();
        journal.append("topic", None, Bytes::from("msg1")).await.unwrap();
        journal.append("topic", None, Bytes::from("msg2")).await.unwrap();
        let mut reader = journal.reader("topic", JournalPosition::Beginning).await.unwrap();
        let e1 = reader.next().await.unwrap().unwrap();
        let e2 = reader.next().await.unwrap().unwrap();
        assert_eq!(e1.sequence, 1);
        assert_eq!(e2.sequence, 2);
        assert!(reader.next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_end_position() {
        let journal = InMemoryJournal::new();
        assert_eq!(journal.end_position("topic").await.unwrap(), 0);
        journal.append("topic", None, Bytes::from("x")).await.unwrap();
        assert_eq!(journal.end_position("topic").await.unwrap(), 1);
    }
}
