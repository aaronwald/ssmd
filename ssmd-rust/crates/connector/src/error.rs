use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectorError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    #[error("disconnected: {0}")]
    Disconnected(String),
}

#[derive(Error, Debug)]
pub enum WriterError {
    #[error("write failed: {0}")]
    WriteFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum ResolverError {
    #[error("unsupported source: {0}")]
    UnsupportedSource(String),
    #[error("missing key: {0}")]
    MissingKey(String),
}
