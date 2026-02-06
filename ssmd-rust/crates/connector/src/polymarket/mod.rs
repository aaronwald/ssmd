//! Polymarket CLOB exchange connector
//!
//! Provides WebSocket connectivity to Polymarket prediction markets via their CLOB API.
//! No authentication required for public market data channel.

pub mod connector;
pub mod market_discovery;
pub mod messages;
pub mod websocket;
pub mod writer;

pub use connector::PolymarketConnector;
pub use market_discovery::{DiscoveredMarket, MarketDiscovery};
pub use messages::PolymarketWsMessage;
pub use websocket::{PolymarketWebSocket, PolymarketWebSocketError, POLYMARKET_WS_URL};
pub use writer::PolymarketNatsWriter;
