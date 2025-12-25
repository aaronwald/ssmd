//! ssmd-archiver: NATS to file archiver for SSMD market data
//!
//! Subscribes to NATS JetStream and writes JSONL.gz files with
//! configurable rotation interval.

pub mod config;
pub mod error;
pub mod manifest;
pub mod subscriber;
pub mod writer;

pub use config::Config;
pub use error::ArchiverError;
