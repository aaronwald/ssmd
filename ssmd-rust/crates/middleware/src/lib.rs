//! ssmd-middleware: Pluggable middleware abstractions
//!
//! Provides trait-based abstractions for Transport, Storage, Cache, and Journal
//! with in-memory implementations for testing.

pub mod cache;
pub mod error;
pub mod factory;
pub mod journal;
pub mod memory;
pub mod storage;
pub mod transport;

pub use cache::Cache;
pub use error::{CacheError, JournalError, StorageError, TransportError};
pub use factory::{FactoryError, MiddlewareFactory};
pub use journal::{Journal, JournalEntry, JournalPosition, JournalReader, TopicConfig};
pub use memory::{InMemoryCache, InMemoryJournal, InMemoryStorage, InMemoryTransport};
pub use storage::{ObjectMeta, Storage};
pub use transport::{Subscription, Transport, TransportMessage};
