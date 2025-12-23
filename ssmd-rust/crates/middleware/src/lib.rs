//! ssmd-middleware: Pluggable middleware abstractions
//!
//! Provides trait-based abstractions for Transport, Storage, Cache, and Journal
//! with in-memory implementations for testing.

pub mod cache;
pub mod error;
pub mod journal;
pub mod storage;
pub mod transport;

pub use cache::Cache;
pub use error::{CacheError, JournalError, StorageError, TransportError};
pub use journal::{Journal, JournalEntry, JournalPosition, JournalReader, TopicConfig};
pub use storage::{ObjectMeta, Storage};
pub use transport::{Subscription, Transport, TransportMessage};
