use std::collections::HashMap;
use std::time::Duration;

use async_nats::jetstream::{self, Context};
use async_nats::jetstream::stream::{Config, RetentionPolicy, StorageType};
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
    jetstream: Context,
}

impl NatsTransport {
    /// Create a new NatsTransport from an existing client
    pub fn new(client: Client) -> Self {
        let jetstream = jetstream::new(client.clone());
        Self { client, jetstream }
    }

    /// Connect to NATS server and create transport
    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let client = async_nats::connect(url)
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        Ok(Self::new(client))
    }

    /// Get JetStream context for stream operations
    pub fn jetstream(&self) -> &Context {
        &self.jetstream
    }

    /// Create or get a JetStream stream for market data
    pub async fn ensure_stream(
        &self,
        stream_name: &str,
        subjects: Vec<String>,
    ) -> Result<(), TransportError> {
        let config = Config {
            name: stream_name.to_string(),
            subjects,
            retention: RetentionPolicy::Limits,
            storage: StorageType::File,
            max_age: std::time::Duration::from_secs(24 * 60 * 60), // 24 hours
            ..Default::default()
        };

        self.jetstream
            .get_or_create_stream(config)
            .await
            .map_err(|e| TransportError::PublishFailed(format!("stream creation failed: {}", e)))?;

        Ok(())
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

    #[tokio::test]
    #[ignore] // Requires NATS server with JetStream
    async fn test_ensure_stream() {
        let transport = NatsTransport::connect("nats://localhost:4222").await.unwrap();
        let result = transport.ensure_stream(
            "TEST_STREAM",
            vec!["test.>".to_string()],
        ).await;
        assert!(result.is_ok());
    }
}
