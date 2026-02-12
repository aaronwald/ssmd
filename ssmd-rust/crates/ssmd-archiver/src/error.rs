use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArchiverError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("NATS error: {0}")]
    Nats(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
}
