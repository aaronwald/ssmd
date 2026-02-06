//! Kraken exchange connector
//!
//! Provides WebSocket connectivity to Kraken spot markets via the v2 API.

pub mod connector;
pub mod messages;
pub mod websocket;
pub mod writer;

pub use connector::KrakenConnector;
pub use messages::KrakenWsMessage;
pub use websocket::{KrakenWebSocket, KrakenWebSocketError, KRAKEN_WS_URL};
pub use writer::KrakenNatsWriter;
