//! Polymarket CLOB WebSocket message types
//!
//! Defines the message structures for Polymarket's CLOB WebSocket API.
//! Uses `#[serde(tag = "event_type")]` since Polymarket messages have a consistent `event_type` field.
//!
//! All prices are decimal strings ("0.55"), timestamps are Unix milliseconds as strings.

use serde::Deserialize;

/// Incoming WebSocket messages from Polymarket CLOB API
///
/// Polymarket messages have a consistent `event_type` field for dispatch.
/// We use `#[serde(tag = "event_type")]` for reliable deserialization.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "event_type")]
pub enum PolymarketWsMessage {
    /// Full orderbook snapshot
    #[serde(rename = "book")]
    Book {
        asset_id: String,
        market: String,
        timestamp: Option<String>,
        hash: Option<String>,
        #[serde(alias = "buys")]
        bids: Vec<OrderbookLevel>,
        #[serde(alias = "sells")]
        asks: Vec<OrderbookLevel>,
    },

    /// Incremental orderbook price level update
    #[serde(rename = "price_change")]
    PriceChange {
        market: String,
        timestamp: Option<String>,
        price_changes: Vec<PriceChangeItem>,
    },

    /// Trade execution
    #[serde(rename = "last_trade_price")]
    LastTradePrice {
        asset_id: String,
        market: String,
        price: String,
        side: Option<String>,
        size: Option<String>,
        fee_rate_bps: Option<String>,
        timestamp: Option<String>,
    },

    /// Best bid/ask update (requires custom_feature_enabled)
    #[serde(rename = "best_bid_ask")]
    BestBidAsk {
        market: String,
        asset_id: String,
        best_bid: Option<String>,
        best_ask: Option<String>,
        spread: Option<String>,
        timestamp: Option<String>,
    },

    /// New market created (requires custom_feature_enabled)
    #[serde(rename = "new_market")]
    NewMarket {
        market: String,
        #[serde(default)]
        question: Option<String>,
        #[serde(default)]
        slug: Option<String>,
        #[serde(default)]
        assets_ids: Vec<String>,
        #[serde(default)]
        outcomes: Vec<String>,
        timestamp: Option<String>,
    },

    /// Market resolved (requires custom_feature_enabled)
    #[serde(rename = "market_resolved")]
    MarketResolved {
        market: String,
        #[serde(default)]
        winning_asset_id: Option<String>,
        #[serde(default)]
        winning_outcome: Option<String>,
        timestamp: Option<String>,
    },

    /// Tick size change
    #[serde(rename = "tick_size_change")]
    TickSizeChange {
        asset_id: String,
        market: String,
        old_tick_size: Option<String>,
        new_tick_size: Option<String>,
        side: Option<String>,
        timestamp: Option<String>,
    },
}

impl PolymarketWsMessage {
    /// Extract the condition_id (market field) from the message.
    /// Used for NATS subject routing.
    pub fn condition_id(&self) -> &str {
        match self {
            PolymarketWsMessage::Book { market, .. }
            | PolymarketWsMessage::PriceChange { market, .. }
            | PolymarketWsMessage::LastTradePrice { market, .. }
            | PolymarketWsMessage::BestBidAsk { market, .. }
            | PolymarketWsMessage::NewMarket { market, .. }
            | PolymarketWsMessage::MarketResolved { market, .. }
            | PolymarketWsMessage::TickSizeChange { market, .. } => market,
        }
    }
}

/// A price level in the orderbook
#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookLevel {
    pub price: String,
    pub size: String,
}

/// An individual price change within a price_change event
#[derive(Debug, Clone, Deserialize)]
pub struct PriceChangeItem {
    pub asset_id: String,
    pub price: String,
    pub size: String,
    pub side: String,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub best_bid: Option<String>,
    #[serde(default)]
    pub best_ask: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOK_MESSAGE: &str = r#"{"event_type":"book","asset_id":"21742633143463906290569050155826241533067272736897614950488156847949938836455","market":"0x1234abcd","timestamp":"1706000000000","hash":"abc123","bids":[{"price":"0.55","size":"1000"},{"price":"0.54","size":"500"}],"asks":[{"price":"0.56","size":"750"}]}"#;

    const LAST_TRADE_PRICE_MESSAGE: &str = r#"{"event_type":"last_trade_price","asset_id":"21742633143463906290569050155826241533067272736897614950488156847949938836455","market":"0x1234abcd","price":"0.55","side":"BUY","size":"100","fee_rate_bps":"0","timestamp":"1706000000000"}"#;

    const PRICE_CHANGE_MESSAGE: &str = r#"{"event_type":"price_change","market":"0x1234abcd","timestamp":"1706000000000","price_changes":[{"asset_id":"21742633143463906290569050155826241533067272736897614950488156847949938836455","price":"0.55","size":"750","side":"BUY","hash":"order123","best_bid":"0.55","best_ask":"0.56"}]}"#;

    const BEST_BID_ASK_MESSAGE: &str = r#"{"event_type":"best_bid_ask","market":"0x1234abcd","asset_id":"21742633143463906290569050155826241533067272736897614950488156847949938836455","best_bid":"0.55","best_ask":"0.56","spread":"0.01","timestamp":"1706000000000"}"#;

    const NEW_MARKET_MESSAGE: &str = r#"{"event_type":"new_market","market":"0x5678efgh","question":"Will X happen?","slug":"will-x-happen","assets_ids":["token_yes","token_no"],"outcomes":["Yes","No"],"timestamp":"1706000000000"}"#;

    const MARKET_RESOLVED_MESSAGE: &str = r#"{"event_type":"market_resolved","market":"0x1234abcd","winning_asset_id":"token_yes","winning_outcome":"Yes","timestamp":"1706000000000"}"#;

    const TICK_SIZE_CHANGE_MESSAGE: &str = r#"{"event_type":"tick_size_change","asset_id":"21742633143463906290569050155826241533067272736897614950488156847949938836455","market":"0x1234abcd","old_tick_size":"0.01","new_tick_size":"0.001","side":"BUY","timestamp":"1706000000000"}"#;

    #[test]
    fn test_parse_book_message() {
        let msg: PolymarketWsMessage =
            serde_json::from_str(BOOK_MESSAGE).expect("Failed to parse book");

        match msg {
            PolymarketWsMessage::Book {
                asset_id,
                market,
                bids,
                asks,
                ..
            } => {
                assert!(asset_id.starts_with("21742"));
                assert_eq!(market, "0x1234abcd");
                assert_eq!(bids.len(), 2);
                assert_eq!(bids[0].price, "0.55");
                assert_eq!(bids[0].size, "1000");
                assert_eq!(asks.len(), 1);
                assert_eq!(asks[0].price, "0.56");
            }
            _ => panic!("Expected Book variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_last_trade_price() {
        let msg: PolymarketWsMessage =
            serde_json::from_str(LAST_TRADE_PRICE_MESSAGE).expect("Failed to parse trade");

        match msg {
            PolymarketWsMessage::LastTradePrice {
                asset_id,
                market,
                price,
                side,
                size,
                ..
            } => {
                assert!(asset_id.starts_with("21742"));
                assert_eq!(market, "0x1234abcd");
                assert_eq!(price, "0.55");
                assert_eq!(side, Some("BUY".to_string()));
                assert_eq!(size, Some("100".to_string()));
            }
            _ => panic!("Expected LastTradePrice variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_price_change() {
        let msg: PolymarketWsMessage =
            serde_json::from_str(PRICE_CHANGE_MESSAGE).expect("Failed to parse price change");

        match msg {
            PolymarketWsMessage::PriceChange {
                market,
                price_changes,
                ..
            } => {
                assert_eq!(market, "0x1234abcd");
                assert_eq!(price_changes.len(), 1);
                assert_eq!(price_changes[0].price, "0.55");
                assert_eq!(price_changes[0].side, "BUY");
                assert_eq!(
                    price_changes[0].best_bid,
                    Some("0.55".to_string())
                );
            }
            _ => panic!("Expected PriceChange variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_best_bid_ask() {
        let msg: PolymarketWsMessage =
            serde_json::from_str(BEST_BID_ASK_MESSAGE).expect("Failed to parse best bid ask");

        match msg {
            PolymarketWsMessage::BestBidAsk {
                market,
                best_bid,
                best_ask,
                spread,
                ..
            } => {
                assert_eq!(market, "0x1234abcd");
                assert_eq!(best_bid, Some("0.55".to_string()));
                assert_eq!(best_ask, Some("0.56".to_string()));
                assert_eq!(spread, Some("0.01".to_string()));
            }
            _ => panic!("Expected BestBidAsk variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_new_market() {
        let msg: PolymarketWsMessage =
            serde_json::from_str(NEW_MARKET_MESSAGE).expect("Failed to parse new market");

        match msg {
            PolymarketWsMessage::NewMarket {
                market,
                question,
                outcomes,
                assets_ids,
                ..
            } => {
                assert_eq!(market, "0x5678efgh");
                assert_eq!(question, Some("Will X happen?".to_string()));
                assert_eq!(outcomes, vec!["Yes", "No"]);
                assert_eq!(assets_ids, vec!["token_yes", "token_no"]);
            }
            _ => panic!("Expected NewMarket variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_market_resolved() {
        let msg: PolymarketWsMessage =
            serde_json::from_str(MARKET_RESOLVED_MESSAGE).expect("Failed to parse resolved");

        match msg {
            PolymarketWsMessage::MarketResolved {
                market,
                winning_outcome,
                ..
            } => {
                assert_eq!(market, "0x1234abcd");
                assert_eq!(winning_outcome, Some("Yes".to_string()));
            }
            _ => panic!("Expected MarketResolved variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_tick_size_change() {
        let msg: PolymarketWsMessage =
            serde_json::from_str(TICK_SIZE_CHANGE_MESSAGE).expect("Failed to parse tick size");

        match msg {
            PolymarketWsMessage::TickSizeChange {
                market,
                old_tick_size,
                new_tick_size,
                ..
            } => {
                assert_eq!(market, "0x1234abcd");
                assert_eq!(old_tick_size, Some("0.01".to_string()));
                assert_eq!(new_tick_size, Some("0.001".to_string()));
            }
            _ => panic!("Expected TickSizeChange variant, got {:?}", msg),
        }
    }

    #[test]
    fn test_condition_id_extraction() {
        let msg: PolymarketWsMessage =
            serde_json::from_str(LAST_TRADE_PRICE_MESSAGE).unwrap();
        assert_eq!(msg.condition_id(), "0x1234abcd");
    }

    #[test]
    fn test_parse_minimal_trade() {
        // Minimal trade with only required fields
        let json = r#"{"event_type":"last_trade_price","asset_id":"abc","market":"0xdef","price":"0.50"}"#;
        let msg: PolymarketWsMessage = serde_json::from_str(json).expect("Failed to parse minimal trade");
        match msg {
            PolymarketWsMessage::LastTradePrice { price, side, size, .. } => {
                assert_eq!(price, "0.50");
                assert!(side.is_none());
                assert!(size.is_none());
            }
            _ => panic!("Expected LastTradePrice"),
        }
    }
}
