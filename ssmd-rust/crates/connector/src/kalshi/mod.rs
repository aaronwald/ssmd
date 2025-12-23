//! Kalshi exchange connector
//!
//! Provides WebSocket connectivity to Kalshi prediction markets.

pub mod auth;
pub mod config;
pub mod connector;
pub mod messages;
pub mod websocket;

pub use auth::{AuthError, KalshiCredentials};
pub use config::{ConfigError, KalshiConfig};
pub use connector::KalshiConnector;
pub use messages::{OrderbookData, TickerData, TradeData, WsMessage};
pub use websocket::{KalshiWebSocket, WebSocketError, KALSHI_WS_DEMO_URL, KALSHI_WS_URL};
