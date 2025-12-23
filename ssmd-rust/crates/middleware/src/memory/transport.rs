use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::error::TransportError;
use crate::latency::now_tsc;
use crate::transport::{Subscription, Transport, TransportMessage};

const CHANNEL_BUFFER_SIZE: usize = 1024;

pub struct InMemoryTransport {
    channels: DashMap<String, broadcast::Sender<TransportMessage>>,
    sequence: AtomicU64,
}

impl InMemoryTransport {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
            sequence: AtomicU64::new(0),
        }
    }

    #[inline]
    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    fn get_or_create_channel(&self, subject: &str) -> broadcast::Sender<TransportMessage> {
        self.channels
            .entry(subject.to_string())
            .or_insert_with(|| broadcast::channel(CHANNEL_BUFFER_SIZE).0)
            .clone()
    }
}

impl Default for InMemoryTransport {
    fn default() -> Self {
        Self::new()
    }
}

struct InMemorySubscription {
    rx: broadcast::Receiver<TransportMessage>,
}

#[async_trait]
impl Subscription for InMemorySubscription {
    async fn next(&mut self) -> Result<TransportMessage, TransportError> {
        self.rx
            .recv()
            .await
            .map_err(|e| TransportError::SubscribeFailed(e.to_string()))
    }

    async fn ack(&self, _sequence: u64) -> Result<(), TransportError> {
        Ok(())
    }

    async fn unsubscribe(self: Box<Self>) -> Result<(), TransportError> {
        Ok(())
    }
}

#[async_trait]
impl Transport for InMemoryTransport {
    async fn publish(&self, subject: &str, payload: Bytes) -> Result<(), TransportError> {
        self.publish_with_headers(subject, payload, HashMap::new())
            .await
    }

    async fn publish_with_headers(
        &self,
        subject: &str,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<(), TransportError> {
        let tx = self.get_or_create_channel(subject);
        let seq = self.next_sequence();
        let msg = TransportMessage {
            subject: subject.to_string(),
            payload,
            headers,
            timestamp: now_tsc(),
            sequence: Some(seq),
        };
        let _ = tx.send(msg);
        Ok(())
    }

    async fn subscribe(&self, subject: &str) -> Result<Box<dyn Subscription>, TransportError> {
        let tx = self.get_or_create_channel(subject);
        let rx = tx.subscribe();
        Ok(Box::new(InMemorySubscription { rx }))
    }

    async fn request(
        &self,
        _subject: &str,
        _payload: Bytes,
        _timeout: Duration,
    ) -> Result<TransportMessage, TransportError> {
        Err(TransportError::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let transport = InMemoryTransport::new();
        let mut sub = transport.subscribe("test.subject").await.unwrap();
        transport
            .publish("test.subject", Bytes::from("hello"))
            .await
            .unwrap();
        let msg = sub.next().await.unwrap();
        assert_eq!(msg.subject, "test.subject");
        assert_eq!(msg.payload, Bytes::from("hello"));
    }

    #[tokio::test]
    async fn test_sequence_numbers_increment() {
        let transport = InMemoryTransport::new();
        let mut sub = transport.subscribe("test.seq").await.unwrap();
        transport
            .publish("test.seq", Bytes::from("1"))
            .await
            .unwrap();
        transport
            .publish("test.seq", Bytes::from("2"))
            .await
            .unwrap();
        let msg1 = sub.next().await.unwrap();
        let msg2 = sub.next().await.unwrap();
        assert_eq!(msg1.sequence, Some(0));
        assert_eq!(msg2.sequence, Some(1));
    }

    #[tokio::test]
    async fn test_timestamp_is_tsc() {
        let transport = InMemoryTransport::new();
        let mut sub = transport.subscribe("test.ts").await.unwrap();

        let before = now_tsc();
        transport.publish("test.ts", Bytes::from("x")).await.unwrap();
        let after = now_tsc();

        let msg = sub.next().await.unwrap();
        assert!(msg.timestamp >= before && msg.timestamp <= after);
    }
}
