use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::error::{ConnectorError, ResolverError, WriterError};
use crate::message::Message;

/// Timestamped message: (tsc, raw_bytes)
/// TSC is captured at WebSocket receive time for accurate end-to-end latency.
pub type TimestampedMsg = (u64, Vec<u8>);

/// Connector trait for data sources (WebSocket, REST, etc.)
#[async_trait]
pub trait Connector: Send + Sync {
    /// Establish connection to the data source
    async fn connect(&mut self) -> Result<(), ConnectorError>;

    /// Get receiver for incoming messages (with TSC timestamp from WS receive)
    fn messages(&mut self) -> mpsc::Receiver<TimestampedMsg>;

    /// Close the connection
    async fn close(&mut self) -> Result<(), ConnectorError>;

    /// Get handle to last WebSocket activity timestamp (epoch seconds).
    /// Used for health checks - returns None if connector doesn't track activity.
    /// Activity includes both ping/pong and data messages.
    ///
    /// Any received frame updates this, including ones that are dropped rather than
    /// forwarded. It answers "is this socket responding at all", not "is this feed
    /// producing data" — see [`Connector::data_handle`] for the latter.
    fn activity_handle(&self) -> Option<Arc<AtomicU64>> {
        None
    }

    /// Get handle to the last timestamp (epoch seconds) at which this connector
    /// forwarded a message downstream, 0 before the connection is up.
    ///
    /// Distinct from [`Connector::activity_handle`], which any received frame
    /// updates. The absolute data-age ceiling reads THIS clock, because control
    /// frames — subscribe and unsubscribe acks, `ok`, `error`, unrecognised types —
    /// come from the peer, and a ceiling a peer can reset is not a backstop
    /// against that peer going silent.
    ///
    /// Returns None if the connector does not track it separately, in which case
    /// the runner substitutes its own clock, stamped on every successful write.
    /// Implementors must stamp it once when the connection comes up: leaving it at
    /// 0 until the first message reads as "uninitialized" and suppresses the
    /// ceiling for a connector that never publishes anything at all.
    fn data_handle(&self) -> Option<Arc<AtomicU64>> {
        None
    }

    /// Get handle to the last WebSocket pong timestamp (epoch seconds), 0 if none yet.
    /// Health checks combine this with the activity handle: a connection is alive if
    /// EITHER data or a pong arrived recently, so feeds that go legitimately quiet are
    /// not killed while their socket is healthy.
    /// Returns None if the connector does not track pongs, in which case callers fall
    /// back to data age alone.
    fn pong_handle(&self) -> Option<Arc<AtomicU64>> {
        None
    }

    /// Take ownership of spawned background tasks (shard receivers, CDC consumer, etc.).
    /// The runner monitors these via JoinSet — if any task exits or panics, the
    /// connector crashes instead of silently losing data.
    fn tasks(&mut self) -> Option<JoinSet<()>> {
        None
    }
}

/// Writer trait for output destinations (file, S3, NATS, etc.)
#[async_trait]
pub trait Writer: Send + Sync {
    /// Write a message to the destination
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError>;

    /// Close and flush the writer
    async fn close(&mut self) -> Result<(), WriterError>;
}

/// KeyResolver trait for credential sources (env vars, Vault, etc.)
pub trait KeyResolver: Send + Sync {
    /// Resolve keys from a source string (e.g., "env:VAR1,VAR2")
    fn resolve(&self, source: &str) -> Result<HashMap<String, String>, ResolverError>;
}
