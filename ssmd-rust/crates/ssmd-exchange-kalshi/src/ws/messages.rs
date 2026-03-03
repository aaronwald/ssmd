//! Kalshi WebSocket message types for private trading channels.
//!
//! Covers: `fill`, `user_order`, `market_position`, `market_lifecycle_v2`.
//! Unit conversions are handled during conversion to `ExchangeEvent`.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use harman::exchange::ExchangeEvent;
use harman::state::OrderState;
use harman::types::{Action, MarketResult, Side};

// ---------------------------------------------------------------------------
// Top-level WS message envelope
// ---------------------------------------------------------------------------

/// Incoming private WebSocket messages from Kalshi.
///
/// Uses serde's internally-tagged enum on the `"type"` field.
/// Unknown message types are captured as `Unknown` to avoid parse failures
/// when Kalshi adds new channels.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum WsPrivateMessage {
    Fill {
        #[serde(default)]
        sid: Option<u64>,
        msg: FillData,
    },
    UserOrder {
        #[serde(default)]
        sid: Option<u64>,
        msg: UserOrderData,
    },
    MarketPosition {
        #[serde(default)]
        sid: Option<u64>,
        msg: MarketPositionData,
    },
    MarketLifecycleV2 {
        #[serde(default)]
        sid: Option<u64>,
        msg: MarketLifecycleData,
    },
    /// Subscription confirmations
    Subscribed {
        id: u64,
        #[serde(default)]
        msg: Option<SubscribedData>,
    },
    Ok {
        id: u64,
        #[serde(default)]
        sid: Option<u64>,
    },
    Error {
        id: Option<u64>,
        #[serde(default)]
        msg: Option<ErrorData>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscribedData {
    pub channel: String,
    #[serde(default)]
    pub sid: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorData {
    pub code: i64,
    pub msg: String,
}

// ---------------------------------------------------------------------------
// Fill channel
// ---------------------------------------------------------------------------

/// Fill event from the `fill` private channel.
///
/// `yes_price` is in cents (1-99). `ts` is Unix seconds.
#[derive(Debug, Clone, Deserialize)]
pub struct FillData {
    pub trade_id: String,
    pub order_id: String,
    pub market_ticker: String,
    pub is_taker: bool,
    pub side: String,
    pub yes_price: i64,
    #[serde(default)]
    pub count: Option<i64>,
    #[serde(default)]
    pub count_fp: Option<String>,
    pub action: String,
    pub ts: i64,
    #[serde(default)]
    pub client_order_id: Option<String>,
}

impl FillData {
    /// Convert to `ExchangeEvent::Fill`.
    ///
    /// `yes_price` (cents) → divide by 100 for dollars.
    /// `count_fp` is preferred over `count` for quantity.
    pub fn to_exchange_event(&self) -> Option<ExchangeEvent> {
        let price_dollars = Decimal::new(self.yes_price, 2);
        let quantity = self
            .count_fp
            .as_ref()
            .and_then(|s| s.parse::<Decimal>().ok())
            .or_else(|| self.count.map(Decimal::from))
            .unwrap_or(Decimal::ZERO);
        let side = parse_side(&self.side)?;
        let action = parse_action(&self.action)?;
        let filled_at = DateTime::from_timestamp(self.ts, 0)?;
        let client_order_id = self
            .client_order_id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok());

        Some(ExchangeEvent::Fill {
            trade_id: self.trade_id.clone(),
            exchange_order_id: self.order_id.clone(),
            ticker: self.market_ticker.clone(),
            side,
            action,
            price_dollars,
            quantity,
            is_taker: self.is_taker,
            filled_at,
            client_order_id,
        })
    }
}

// ---------------------------------------------------------------------------
// User orders channel
// ---------------------------------------------------------------------------

/// Order update from the `user_order` private channel.
///
/// All price/cost fields are dollar strings. Counts are fixed-point strings.
#[derive(Debug, Clone, Deserialize)]
pub struct UserOrderData {
    pub order_id: String,
    #[serde(default)]
    pub client_order_id: Option<String>,
    pub ticker: String,
    pub status: String,
    #[serde(default)]
    pub fill_count_fp: Option<String>,
    #[serde(default)]
    pub remaining_count_fp: Option<String>,
    #[serde(default)]
    pub close_cancel_count: Option<i64>,
}

impl UserOrderData {
    /// Convert to `ExchangeEvent::OrderUpdate`.
    pub fn to_exchange_event(&self) -> Option<ExchangeEvent> {
        let status = match self.status.as_str() {
            "resting" => OrderState::Acknowledged,
            "executed" => OrderState::Filled,
            "canceled" | "cancelled" => OrderState::Cancelled,
            _ => return None,
        };

        let filled_quantity = self
            .fill_count_fp
            .as_ref()
            .and_then(|s| s.parse::<Decimal>().ok())
            .unwrap_or(Decimal::ZERO);

        let remaining_quantity = self
            .remaining_count_fp
            .as_ref()
            .and_then(|s| s.parse::<Decimal>().ok())
            .unwrap_or(Decimal::ZERO);

        let client_order_id = self
            .client_order_id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok());

        Some(ExchangeEvent::OrderUpdate {
            exchange_order_id: self.order_id.clone(),
            client_order_id,
            ticker: self.ticker.clone(),
            status,
            filled_quantity,
            remaining_quantity,
            close_cancel_count: self.close_cancel_count,
        })
    }
}

// ---------------------------------------------------------------------------
// Market positions channel
// ---------------------------------------------------------------------------

/// Position update from the `market_position` private channel.
///
/// `position_cost`, `realized_pnl`, `fees_paid` are in **centi-cents**.
/// Divide by 10,000 to get dollars.
#[derive(Debug, Clone, Deserialize)]
pub struct MarketPositionData {
    pub market_ticker: String,
    #[serde(default)]
    pub position: Option<i64>,
    #[serde(default)]
    pub position_fp: Option<String>,
    #[serde(default)]
    pub position_cost: Option<i64>,
    #[serde(default)]
    pub realized_pnl: Option<i64>,
    #[serde(default)]
    pub volume: Option<i64>,
    #[serde(default)]
    pub volume_fp: Option<String>,
}

impl MarketPositionData {
    /// Convert to `ExchangeEvent::PositionUpdate`.
    ///
    /// Centi-cents → dollars: divide by 10,000.
    pub fn to_exchange_event(&self) -> ExchangeEvent {
        let position = self
            .position_fp
            .as_ref()
            .and_then(|s| s.parse::<Decimal>().ok())
            .or_else(|| self.position.map(Decimal::from))
            .unwrap_or(Decimal::ZERO);

        let position_cost_dollars = self
            .position_cost
            .map(|c| Decimal::new(c, 4))
            .unwrap_or(Decimal::ZERO);

        let realized_pnl_dollars = self
            .realized_pnl
            .map(|c| Decimal::new(c, 4))
            .unwrap_or(Decimal::ZERO);

        let volume = self
            .volume_fp
            .as_ref()
            .and_then(|s| s.parse::<Decimal>().ok())
            .or_else(|| self.volume.map(Decimal::from))
            .unwrap_or(Decimal::ZERO);

        ExchangeEvent::PositionUpdate {
            ticker: self.market_ticker.clone(),
            position,
            position_cost_dollars,
            realized_pnl_dollars,
            volume,
        }
    }
}

// ---------------------------------------------------------------------------
// Market lifecycle channel (determined/settled events)
// ---------------------------------------------------------------------------

/// Lifecycle event from the `market_lifecycle_v2` channel.
///
/// We only care about `determined` and `settled` event types for harman.
#[derive(Debug, Clone, Deserialize)]
pub struct MarketLifecycleData {
    pub market_ticker: String,
    pub event_type: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub determination_ts: Option<i64>,
    #[serde(default)]
    pub settled_ts: Option<i64>,
}

impl MarketLifecycleData {
    /// Convert to `ExchangeEvent::MarketSettled` if this is a settlement event.
    /// Returns None for non-settlement events (created, activated, etc.).
    pub fn to_exchange_event(&self) -> Option<ExchangeEvent> {
        match self.event_type.as_str() {
            "determined" | "settled" => {
                let result = match self.result.as_deref() {
                    Some("yes") => MarketResult::Yes,
                    Some("no") => MarketResult::No,
                    _ => MarketResult::Unknown,
                };

                let ts = self
                    .determination_ts
                    .or(self.settled_ts)
                    .and_then(|t| DateTime::from_timestamp(t, 0))
                    .unwrap_or_else(Utc::now);

                Some(ExchangeEvent::MarketSettled {
                    ticker: self.market_ticker.clone(),
                    result,
                    settled_time: ts,
                })
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_side(s: &str) -> Option<Side> {
    match s {
        "yes" => Some(Side::Yes),
        "no" => Some(Side::No),
        _ => None,
    }
}

fn parse_action(s: &str) -> Option<Action> {
    match s {
        "buy" => Some(Action::Buy),
        "sell" => Some(Action::Sell),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const FILL_MSG: &str = r#"{
        "type": "fill",
        "sid": 2,
        "msg": {
            "trade_id": "550e8400-e29b-41d4-a716-446655440000",
            "order_id": "660e8400-e29b-41d4-a716-446655440001",
            "market_ticker": "KXBTCD-26MAR0620-T105000",
            "is_taker": true,
            "side": "yes",
            "yes_price": 62,
            "count": 10,
            "count_fp": "10.00",
            "action": "buy",
            "ts": 1740768000,
            "client_order_id": "abc12345-e29b-41d4-a716-446655440099"
        }
    }"#;

    #[test]
    fn test_parse_fill() {
        let msg: WsPrivateMessage = serde_json::from_str(FILL_MSG).unwrap();
        match msg {
            WsPrivateMessage::Fill { sid, msg } => {
                assert_eq!(sid, Some(2));
                assert_eq!(msg.market_ticker, "KXBTCD-26MAR0620-T105000");
                assert_eq!(msg.yes_price, 62);
                assert!(msg.is_taker);
                assert_eq!(msg.action, "buy");

                let event = msg.to_exchange_event().unwrap();
                match event {
                    ExchangeEvent::Fill {
                        price_dollars,
                        quantity,
                        ..
                    } => {
                        assert_eq!(price_dollars, Decimal::new(62, 2)); // $0.62
                        assert_eq!(quantity, Decimal::new(1000, 2)); // 10.00
                    }
                    _ => panic!("expected Fill event"),
                }
            }
            _ => panic!("expected Fill variant"),
        }
    }

    const USER_ORDER_MSG: &str = r#"{
        "type": "user_order",
        "sid": 2,
        "msg": {
            "order_id": "660e8400-e29b-41d4-a716-446655440001",
            "client_order_id": "abc12345-e29b-41d4-a716-446655440099",
            "ticker": "KXBTCD-26MAR0620-T105000",
            "status": "resting",
            "fill_count_fp": "5.00",
            "remaining_count_fp": "95.00"
        }
    }"#;

    #[test]
    fn test_parse_user_order() {
        let msg: WsPrivateMessage = serde_json::from_str(USER_ORDER_MSG).unwrap();
        match msg {
            WsPrivateMessage::UserOrder { msg, .. } => {
                assert_eq!(msg.status, "resting");
                let event = msg.to_exchange_event().unwrap();
                match event {
                    ExchangeEvent::OrderUpdate {
                        status,
                        filled_quantity,
                        remaining_quantity,
                        ..
                    } => {
                        assert_eq!(status, OrderState::Acknowledged);
                        assert_eq!(filled_quantity, Decimal::new(500, 2));
                        assert_eq!(remaining_quantity, Decimal::new(9500, 2));
                    }
                    _ => panic!("expected OrderUpdate event"),
                }
            }
            _ => panic!("expected UserOrder variant"),
        }
    }

    #[test]
    fn test_parse_user_order_cancelled() {
        let json = r#"{
            "type": "user_order",
            "sid": 2,
            "msg": {
                "order_id": "order-1",
                "ticker": "TEST",
                "status": "canceled",
                "fill_count_fp": "0.00",
                "remaining_count_fp": "0.00",
                "close_cancel_count": 10
            }
        }"#;
        let msg: WsPrivateMessage = serde_json::from_str(json).unwrap();
        match msg {
            WsPrivateMessage::UserOrder { msg, .. } => {
                assert_eq!(msg.close_cancel_count, Some(10));
                let event = msg.to_exchange_event().unwrap();
                match event {
                    ExchangeEvent::OrderUpdate {
                        status,
                        close_cancel_count,
                        ..
                    } => {
                        assert_eq!(status, OrderState::Cancelled);
                        assert_eq!(close_cancel_count, Some(10));
                    }
                    _ => panic!("expected OrderUpdate"),
                }
            }
            _ => panic!("expected UserOrder"),
        }
    }

    const POSITION_MSG: &str = r#"{
        "type": "market_position",
        "sid": 3,
        "msg": {
            "market_ticker": "KXBTCD-26MAR0620-T105000",
            "position": 10,
            "position_fp": "10.00",
            "position_cost": 550000,
            "realized_pnl": -12500,
            "volume": 25,
            "volume_fp": "25.00"
        }
    }"#;

    #[test]
    fn test_parse_market_position() {
        let msg: WsPrivateMessage = serde_json::from_str(POSITION_MSG).unwrap();
        match msg {
            WsPrivateMessage::MarketPosition { msg, .. } => {
                let event = msg.to_exchange_event();
                match event {
                    ExchangeEvent::PositionUpdate {
                        position,
                        position_cost_dollars,
                        realized_pnl_dollars,
                        volume,
                        ..
                    } => {
                        assert_eq!(position, Decimal::new(1000, 2)); // 10.00
                        assert_eq!(position_cost_dollars, Decimal::new(550000, 4)); // $55.00
                        assert_eq!(realized_pnl_dollars, Decimal::new(-12500, 4)); // -$1.25
                        assert_eq!(volume, Decimal::new(2500, 2)); // 25.00
                    }
                    _ => panic!("expected PositionUpdate"),
                }
            }
            _ => panic!("expected MarketPosition"),
        }
    }

    const LIFECYCLE_DETERMINED: &str = r#"{
        "type": "market_lifecycle_v2",
        "sid": 13,
        "msg": {
            "market_ticker": "KXBTCD-26MAR0620-T105000",
            "event_type": "determined",
            "result": "yes",
            "determination_ts": 1740768000
        }
    }"#;

    #[test]
    fn test_parse_lifecycle_determined() {
        let msg: WsPrivateMessage = serde_json::from_str(LIFECYCLE_DETERMINED).unwrap();
        match msg {
            WsPrivateMessage::MarketLifecycleV2 { msg, .. } => {
                let event = msg.to_exchange_event().unwrap();
                match event {
                    ExchangeEvent::MarketSettled { result, .. } => {
                        assert_eq!(result, MarketResult::Yes);
                    }
                    _ => panic!("expected MarketSettled"),
                }
            }
            _ => panic!("expected MarketLifecycleV2"),
        }
    }

    #[test]
    fn test_lifecycle_activated_ignored() {
        let json = r#"{
            "type": "market_lifecycle_v2",
            "sid": 13,
            "msg": {
                "market_ticker": "TEST",
                "event_type": "activated"
            }
        }"#;
        let msg: WsPrivateMessage = serde_json::from_str(json).unwrap();
        match msg {
            WsPrivateMessage::MarketLifecycleV2 { msg, .. } => {
                assert!(msg.to_exchange_event().is_none());
            }
            _ => panic!("expected MarketLifecycleV2"),
        }
    }

    #[test]
    fn test_unknown_message() {
        let json = r#"{"type": "some_future_type", "data": 42}"#;
        let msg: WsPrivateMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, WsPrivateMessage::Unknown));
    }

    #[test]
    fn test_subscribed_message() {
        let json = r#"{"type": "subscribed", "id": 1, "msg": {"channel": "fill", "sid": 42}}"#;
        let msg: WsPrivateMessage = serde_json::from_str(json).unwrap();
        match msg {
            WsPrivateMessage::Subscribed { id, msg } => {
                assert_eq!(id, 1);
                let data = msg.unwrap();
                assert_eq!(data.channel, "fill");
                assert_eq!(data.sid, Some(42));
            }
            _ => panic!("expected Subscribed"),
        }
    }
}
