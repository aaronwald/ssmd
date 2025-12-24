use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_nats::Client;
use async_trait::async_trait;
use bytes::Bytes;

use crate::error::TransportError;
use crate::latency::now_tsc;
use crate::transport::{Subscription, Transport, TransportMessage};

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
