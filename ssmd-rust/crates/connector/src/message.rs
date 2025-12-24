use ssmd_middleware::now_tsc;

/// Message wraps raw data with metadata.
/// Stores raw bytes to avoid JSON parsing overhead in hot path.
#[derive(Debug, Clone)]
pub struct Message {
    /// TSC timestamp (zero-syscall, convert to wall clock at I/O boundary)
    pub tsc: u64,
    /// Feed name (shared reference to avoid allocation)
    pub feed: String,
    /// Raw message bytes (no parsing in hot path)
    pub data: Vec<u8>,
}

impl Message {
    /// Create a new message with raw bytes.
    /// Uses TSC timestamp to avoid syscall overhead.
    #[inline]
    pub fn new(feed: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            tsc: now_tsc(),
            feed: feed.into(),
            data,
        }
    }

    /// Create message from borrowed data (copies bytes).
    #[inline]
    pub fn from_slice(feed: impl Into<String>, data: &[u8]) -> Self {
        Self {
            tsc: now_tsc(),
            feed: feed.into(),
            data: data.to_vec(),
        }
    }
}
