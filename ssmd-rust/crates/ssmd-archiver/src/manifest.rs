use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub feed: String,
    pub date: String,
    pub format: String,
    pub rotation_interval: String,
    pub files: Vec<FileEntry>,
    pub gaps: Vec<Gap>,
    pub tickers: Vec<String>,
    pub message_types: Vec<String>,
    pub has_gaps: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub records: u64,
    pub bytes: u64,
    pub nats_start_seq: u64,
    pub nats_end_seq: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Gap {
    pub after_seq: u64,
    pub missing_count: u64,
    pub detected_at: DateTime<Utc>,
}

impl Manifest {
    pub fn new(feed: &str, date: &str, rotation_interval: &str) -> Self {
        Self {
            feed: feed.to_string(),
            date: date.to_string(),
            format: "jsonl".to_string(),
            rotation_interval: rotation_interval.to_string(),
            files: Vec::new(),
            gaps: Vec::new(),
            tickers: Vec::new(),
            message_types: Vec::new(),
            has_gaps: false,
        }
    }
}
