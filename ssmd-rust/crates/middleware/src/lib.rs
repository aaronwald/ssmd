//! ssmd-middleware: Pluggable middleware abstractions
//!
//! Provides trait-based abstractions for Transport, Storage, Cache, and Journal
//! with in-memory implementations for testing.

pub mod error;
pub mod transport;

pub use error::{CacheError, JournalError, StorageError, TransportError};
pub use transport::{Subscription, Transport, TransportMessage};
