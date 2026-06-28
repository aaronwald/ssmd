//! Binance spot exchange connector
//!
//! Provides WebSocket connectivity to Binance spot markets via the public
//! combined-stream API on the `data-stream.binance.vision` market-data mirror.
//! No authentication is required for public `@trade` streams.

pub mod connector;
pub mod messages;
pub mod websocket;
pub mod writer;

pub use connector::BinanceConnector;
pub use messages::BinanceWsMessage;
pub use websocket::{BinanceWebSocket, BinanceWebSocketError, BINANCE_WS_URL};
pub use writer::BinanceNatsWriter;
