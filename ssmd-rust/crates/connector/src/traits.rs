use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::error::{ConnectorError, ResolverError, WriterError};
use crate::message::Message;

/// Connector trait for data sources (WebSocket, REST, etc.)
#[async_trait]
pub trait Connector: Send + Sync {
    /// Establish connection to the data source
    async fn connect(&mut self) -> Result<(), ConnectorError>;

    /// Get receiver for incoming messages
    fn messages(&mut self) -> mpsc::Receiver<Vec<u8>>;

    /// Close the connection
    async fn close(&mut self) -> Result<(), ConnectorError>;

    /// Get handle to last WebSocket activity timestamp (epoch seconds).
    /// Used for health checks - returns None if connector doesn't track activity.
    /// Activity includes both ping/pong and data messages.
    fn activity_handle(&self) -> Option<Arc<AtomicU64>> {
        None
    }
}

/// Writer trait for output destinations (file, S3, NATS, etc.)
#[async_trait]
pub trait Writer: Send + Sync {
    /// Write a message to the destination
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError>;

    /// Close and flush the writer
    async fn close(&mut self) -> Result<(), WriterError>;
}

/// KeyResolver trait for credential sources (env vars, Vault, etc.)
pub trait KeyResolver: Send + Sync {
    /// Resolve keys from a source string (e.g., "env:VAR1,VAR2")
    fn resolve(&self, source: &str) -> Result<HashMap<String, String>, ResolverError>;
}
