//! Kalshi exchange connector
//!
//! Provides WebSocket connectivity to Kalshi prediction markets.

pub mod auth;
pub mod cdc_consumer;
pub mod config;
pub mod connector;
pub mod messages;
pub mod shard_manager;
pub mod websocket;

pub use auth::{AuthError, KalshiCredentials};
pub use cdc_consumer::{CdcConfig, CdcError, CdcSubscriptionConsumer};
pub use config::{ConfigError, KalshiConfig};
pub use connector::{KalshiConnector, ShardCommand};
pub use messages::{OrderbookData, TickerData, TradeData, WsMessage};
pub use shard_manager::ShardManager;
pub use websocket::{
    KalshiWebSocket, SubscriptionResult, WebSocketError, KALSHI_WS_DEMO_URL, KALSHI_WS_URL,
};
