//! Kraken Futures exchange connector
//!
//! Provides WebSocket connectivity to Kraken Futures (perpetual contracts) via the v1 API.
//! Separate from the spot connector (kraken/) due to different protocol.

pub mod connector;
pub mod messages;
pub mod websocket;
pub mod writer;

pub use connector::KrakenFuturesConnector;
pub use messages::KrakenFuturesWsMessage;
pub use websocket::{KrakenFuturesWebSocket, KrakenFuturesWsError, KRAKEN_FUTURES_WS_URL};
pub use writer::KrakenFuturesNatsWriter;
