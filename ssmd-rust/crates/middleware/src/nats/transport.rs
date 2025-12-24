use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_nats::Client;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;

use crate::error::TransportError;
use crate::latency::now_tsc;
use crate::transport::{Subscription, Transport, TransportMessage};

/// NATS subscription wrapper
struct NatsSubscription {
    subscriber: async_nats::Subscriber,
}

impl NatsSubscription {
    fn new(subscriber: async_nats::Subscriber) -> Self {
        Self { subscriber }
    }
}

#[async_trait]
impl Subscription for NatsSubscription {
    async fn next(&mut self) -> Result<TransportMessage, TransportError> {
        let msg = self.subscriber
            .next()
            .await
            .ok_or_else(|| TransportError::SubscribeFailed("subscription closed".to_string()))?;

        Ok(TransportMessage {
            subject: msg.subject.to_string(),
            payload: msg.payload,
            headers: HashMap::new(),
            timestamp: now_tsc(),
            sequence: None,
        })
    }

    async fn ack(&self, _sequence: u64) -> Result<(), TransportError> {
        // Core NATS doesn't have ack - JetStream does
        Ok(())
    }

    async fn unsubscribe(mut self: Box<Self>) -> Result<(), TransportError> {
        self.subscriber
            .unsubscribe()
            .await
            .map_err(|e| TransportError::SubscribeFailed(e.to_string()))
    }
}

/// NATS transport implementation
pub struct NatsTransport {
    client: Client,
    sequence: AtomicU64,
}

impl NatsTransport {
    /// Create a new NatsTransport from an existing client
    pub fn new(client: Client) -> Self {
        Self {
            client,
            sequence: AtomicU64::new(0),
        }
    }

    /// Connect to NATS server and create transport
    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let client = async_nats::connect(url)
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        Ok(Self::new(client))
    }

    #[inline]
    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }
}

#[async_trait]
impl Transport for NatsTransport {
    async fn publish(&self, subject: &str, payload: Bytes) -> Result<(), TransportError> {
        self.client
            .publish(subject.to_string(), payload)
            .await
            .map_err(|e| TransportError::PublishFailed(e.to_string()))
    }

    async fn publish_with_headers(
        &self,
        subject: &str,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<(), TransportError> {
        let mut nats_headers = async_nats::HeaderMap::new();
        for (k, v) in headers {
            nats_headers.insert(k, v);
        }

        self.client
            .publish_with_headers(subject.to_string(), nats_headers, payload)
            .await
            .map_err(|e| TransportError::PublishFailed(e.to_string()))
    }

    async fn subscribe(&self, subject: &str) -> Result<Box<dyn Subscription>, TransportError> {
        let subscriber = self.client
            .subscribe(subject.to_string())
            .await
            .map_err(|e| TransportError::SubscribeFailed(e.to_string()))?;
        Ok(Box::new(NatsSubscription::new(subscriber)))
    }

    async fn request(
        &self,
        subject: &str,
        payload: Bytes,
        timeout: Duration,
    ) -> Result<TransportMessage, TransportError> {
        let response = tokio::time::timeout(
            timeout,
            self.client.request(subject.to_string(), payload),
        )
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(|e| TransportError::RequestFailed(e.to_string()))?;

        Ok(TransportMessage {
            subject: response.subject.to_string(),
            payload: response.payload,
            headers: HashMap::new(),
            timestamp: now_tsc(),
            sequence: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running NATS server
    // Run: docker run -p 4222:4222 nats:latest

    #[tokio::test]
    #[ignore] // Requires NATS server
    async fn test_publish_succeeds() {
        let transport = NatsTransport::connect("nats://localhost:4222").await.unwrap();
        let result = transport.publish("test.subject", Bytes::from("hello")).await;
        assert!(result.is_ok());
    }
}
