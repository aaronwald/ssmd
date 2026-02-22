use std::sync::Arc;

use bytes::Bytes;
use ssmd_middleware::now_tsc;

/// Message wraps raw data with metadata.
/// Stores raw bytes to avoid JSON parsing overhead in hot path.
#[derive(Debug, Clone)]
pub struct Message {
    /// TSC timestamp (zero-syscall, convert to wall clock at I/O boundary)
    pub tsc: u64,
    /// Feed name (shared reference to avoid allocation)
    pub feed: Arc<str>,
    /// Raw message bytes (no parsing in hot path)
    pub data: Bytes,
}

impl Message {
    /// Create a new message with raw bytes.
    /// Uses TSC timestamp to avoid syscall overhead.
    #[inline]
    pub fn new(feed: impl Into<Arc<str>>, data: impl Into<Bytes>) -> Self {
        Self {
            tsc: now_tsc(),
            feed: feed.into(),
            data: data.into(),
        }
    }

    /// Create a new message with a provided TSC timestamp.
    #[inline]
    pub fn new_with_tsc(feed: impl Into<Arc<str>>, data: impl Into<Bytes>, tsc: u64) -> Self {
        Self {
            tsc,
            feed: feed.into(),
            data: data.into(),
        }
    }

    /// Create message from borrowed data (copies bytes).
    #[inline]
    pub fn from_slice(feed: impl Into<Arc<str>>, data: &[u8]) -> Self {
        Self {
            tsc: now_tsc(),
            feed: feed.into(),
            data: Bytes::copy_from_slice(data),
        }
    }
}
