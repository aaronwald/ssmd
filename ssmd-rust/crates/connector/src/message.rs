use serde::{Deserialize, Serialize};

/// Message wraps raw data with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// ISO 8601 timestamp
    pub ts: String,
    /// Feed name
    pub feed: String,
    /// Raw message data (stored as raw JSON value)
    pub data: serde_json::Value,
}

impl Message {
    pub fn new(feed: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            ts: chrono::Utc::now().to_rfc3339(),
            feed: feed.into(),
            data,
        }
    }
}
