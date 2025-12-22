use async_trait::async_trait;
use std::collections::HashMap;
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
