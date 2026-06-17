//! Polygon.io ("massive") US-equities connector
//!
//! Implements WebSocket connectivity to `wss://delayed.polygon.io/stocks`
//! for the 85-ticker equity universe. Authentication is via an auth frame
//! (not HTTP headers). Frames arrive as JSON arrays of events keyed by `ev`.

pub mod messages;
pub mod websocket;
pub mod connector;
pub mod writer;

pub use connector::MassiveConnector;
pub use writer::MassiveNatsWriter;
