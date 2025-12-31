//! Error types for ssmd-cdc

use thiserror::Error;

/// Error type for ssmd-cdc operations
#[derive(Error, Debug)]
pub enum Error {
    /// PostgreSQL error
    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    /// NATS error
    #[error("NATS error: {0}")]
    Nats(#[from] async_nats::error::Error<async_nats::ConnectErrorKind>),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Replication error
    #[error("Replication error: {0}")]
    Replication(String),
}
