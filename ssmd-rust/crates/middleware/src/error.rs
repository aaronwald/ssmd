use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("publish failed: {0}")]
    PublishFailed(String),
    #[error("subscribe failed: {0}")]
    SubscribeFailed(String),
    #[error("request failed: {0}")]
    RequestFailed(String),
    #[error("timeout")]
    Timeout,
    #[error("validation failed: {0}")]
    ValidationFailed(String),
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("write failed: {0}")]
    WriteFailed(String),
    #[error("read failed: {0}")]
    ReadFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("operation failed: {0}")]
    OperationFailed(String),
}

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("append failed: {0}")]
    AppendFailed(String),
    #[error("read failed: {0}")]
    ReadFailed(String),
    #[error("topic not found: {0}")]
    TopicNotFound(String),
}
