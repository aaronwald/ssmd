//! ssmd-connector: Market data collection runtime components
//!
//! This crate provides the core components for connecting to market data sources,
//! processing messages, and writing to various destinations.

pub mod error;
pub mod message;
pub mod publisher;
pub mod resolver;
pub mod ring_buffer;
pub mod runner;
pub mod server;
pub mod traits;
pub mod websocket;
pub mod writer;

pub use error::{ConnectorError, ResolverError, WriterError};
pub use message::Message;
pub use publisher::{Publisher, TradeData, TradeSide};
pub use resolver::EnvResolver;
pub use ring_buffer::{RingBuffer, RING_SIZE, RING_SLOTS, SLOT_SIZE};
pub use runner::Runner;
pub use server::{create_router, run_server, ServerState};
pub use traits::{Connector, KeyResolver, Writer};
pub use websocket::WebSocketConnector;
pub use writer::FileWriter;
