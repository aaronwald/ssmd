use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::error::TransportError;
use crate::transport::{Subscription, Transport, TransportMessage};

pub struct InMemoryTransport {
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<TransportMessage>>>>,
    sequence: Arc<Mutex<u64>>,
}

impl InMemoryTransport {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            sequence: Arc::new(Mutex::new(0)),
        }
    }

    async fn get_or_create_channel(&self, subject: &str) -> broadcast::Sender<TransportMessage> {
        let mut channels = self.channels.write().await;
        if let Some(tx) = channels.get(subject) {
            tx.clone()
        } else {
            let (tx, _) = broadcast::channel(1024);
            channels.insert(subject.to_string(), tx.clone());
            tx
        }
    }

    async fn next_sequence(&self) -> u64 {
        let mut seq = self.sequence.lock().await;
        *seq += 1;
        *seq
    }
}

impl Default for InMemoryTransport {
    fn default() -> Self { Self::new() }
}

struct InMemorySubscription {
    rx: broadcast::Receiver<TransportMessage>,
}

#[async_trait]
impl Subscription for InMemorySubscription {
    async fn next(&mut self) -> Result<TransportMessage, TransportError> {
        self.rx.recv().await.map_err(|e| TransportError::SubscribeFailed(e.to_string()))
    }
    async fn ack(&self, _sequence: u64) -> Result<(), TransportError> { Ok(()) }
    async fn unsubscribe(self: Box<Self>) -> Result<(), TransportError> { Ok(()) }
}

#[async_trait]
impl Transport for InMemoryTransport {
    async fn publish(&self, subject: &str, payload: Bytes) -> Result<(), TransportError> {
        self.publish_with_headers(subject, payload, HashMap::new()).await
    }

    async fn publish_with_headers(&self, subject: &str, payload: Bytes, headers: HashMap<String, String>) -> Result<(), TransportError> {
        let tx = self.get_or_create_channel(subject).await;
        let seq = self.next_sequence().await;
        let msg = TransportMessage {
            subject: subject.to_string(),
            payload,
            headers,
            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64,
            sequence: Some(seq),
        };
        let _ = tx.send(msg);
        Ok(())
    }

    async fn subscribe(&self, subject: &str) -> Result<Box<dyn Subscription>, TransportError> {
        let tx = self.get_or_create_channel(subject).await;
        let rx = tx.subscribe();
        Ok(Box::new(InMemorySubscription { rx }))
    }

    async fn request(&self, _subject: &str, _payload: Bytes, _timeout: Duration) -> Result<TransportMessage, TransportError> {
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
        transport.publish("test.subject", Bytes::from("hello")).await.unwrap();
        let msg = sub.next().await.unwrap();
        assert_eq!(msg.subject, "test.subject");
        assert_eq!(msg.payload, Bytes::from("hello"));
    }

    #[tokio::test]
    async fn test_sequence_numbers_increment() {
        let transport = InMemoryTransport::new();
        let mut sub = transport.subscribe("test.seq").await.unwrap();
        transport.publish("test.seq", Bytes::from("1")).await.unwrap();
        transport.publish("test.seq", Bytes::from("2")).await.unwrap();
        let msg1 = sub.next().await.unwrap();
        let msg2 = sub.next().await.unwrap();
        assert_eq!(msg1.sequence, Some(1));
        assert_eq!(msg2.sequence, Some(2));
    }
}
