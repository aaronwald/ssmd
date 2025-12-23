use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::time::Duration;

use crate::error::TransportError;

/// Message envelope with metadata
#[derive(Debug, Clone)]
pub struct TransportMessage {
    pub subject: String,
    pub payload: Bytes,
    pub headers: HashMap<String, String>,
    pub timestamp: u64,
    pub sequence: Option<u64>,
}

/// Subscription handle for receiving messages
#[async_trait]
pub trait Subscription: Send + Sync {
    /// Receive next message (blocks until available)
    async fn next(&mut self) -> Result<TransportMessage, TransportError>;

    /// Acknowledge a message (for reliable delivery)
    async fn ack(&self, sequence: u64) -> Result<(), TransportError>;

    /// Unsubscribe and close
    async fn unsubscribe(self: Box<Self>) -> Result<(), TransportError>;
}

/// Transport abstraction for pub/sub messaging
#[async_trait]
pub trait Transport: Send + Sync {
    /// Publish a message (fire and forget)
    async fn publish(&self, subject: &str, payload: Bytes) -> Result<(), TransportError>;

    /// Publish with headers
    async fn publish_with_headers(
        &self,
        subject: &str,
        payload: Bytes,
        headers: HashMap<String, String>,
    ) -> Result<(), TransportError>;

    /// Subscribe to a subject pattern
    async fn subscribe(&self, subject: &str) -> Result<Box<dyn Subscription>, TransportError>;

    /// Request/reply pattern with timeout
    async fn request(
        &self,
        subject: &str,
        payload: Bytes,
        timeout: Duration,
    ) -> Result<TransportMessage, TransportError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_message_creation() {
        let msg = TransportMessage {
            subject: "kalshi.trade.BTCUSD".to_string(),
            payload: Bytes::from(r#"{"price":100}"#),
            headers: HashMap::new(),
            timestamp: 1703318400000,
            sequence: Some(1),
        };

        assert_eq!(msg.subject, "kalshi.trade.BTCUSD");
        assert_eq!(msg.sequence, Some(1));
    }
}
