use std::collections::HashMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Kalshi-compatible order representation (Serialize for JSON responses).
#[derive(Debug, Clone, Serialize)]
pub struct Order {
    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
    pub ticker: String,
    pub status: String,
    pub side: String,
    pub action: String,
    pub yes_price: i64,
    pub no_price: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_fp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_count_fp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_time: Option<String>,
}

/// Kalshi-compatible fill representation.
#[derive(Debug, Clone, Serialize)]
pub struct Fill {
    pub trade_id: String,
    pub order_id: String,
    pub ticker: String,
    pub side: String,
    pub action: String,
    pub yes_price: i64,
    pub count: i64,
    pub is_taker: bool,
    pub created_time: String,
}

/// Kalshi-compatible position representation.
#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub ticker: String,
    pub position: i64,
    pub market_exposure: i64,
    pub total_traded: i64,
    pub realized_pnl: i64,
    pub resting_orders_count: Option<i64>,
}

/// Incoming order request (Deserialize from harman's KalshiOrderRequest).
/// Some fields are part of the Kalshi API contract but unused by the test exchange.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct OrderRequest {
    pub ticker: String,
    pub client_order_id: String,
    pub side: String,
    pub action: String,
    #[serde(rename = "type")]
    pub order_type: String,
    pub count_fp: f64,
    pub yes_price: i32,
    #[serde(default = "default_tif")]
    pub time_in_force: String,
    #[serde(default)]
    pub subaccount: i32,
}

fn default_tif() -> String {
    "gtc".to_string()
}

/// In-memory exchange state. All mutations go through methods.
pub struct ExchangeState {
    pub orders: HashMap<String, Order>,
    pub fills: Vec<Fill>,
    pub balance: i64,
    next_order_id: u64,
    next_trade_id: u64,
}

impl ExchangeState {
    pub fn new(starting_balance: i64) -> Self {
        Self {
            orders: HashMap::new(),
            fills: Vec::new(),
            balance: starting_balance,
            next_order_id: 1,
            next_trade_id: 1,
        }
    }

    fn next_order_id(&mut self) -> String {
        let id = format!("test-order-{}", self.next_order_id);
        self.next_order_id += 1;
        id
    }

    fn next_trade_id(&mut self) -> String {
        let id = format!("test-trade-{}", self.next_trade_id);
        self.next_trade_id += 1;
        id
    }

    /// Accept an order, immediately fill it, update balance.
    pub fn submit_order(&mut self, req: &OrderRequest) -> Order {
        let order_id = self.next_order_id();
        let trade_id = self.next_trade_id();
        let now = Utc::now().to_rfc3339();
        let count = req.count_fp.round() as i64;
        let yes_price = req.yes_price as i64;
        let no_price = 100 - yes_price;

        let order = Order {
            order_id: order_id.clone(),
            client_order_id: Some(req.client_order_id.clone()),
            ticker: req.ticker.clone(),
            status: "executed".to_string(),
            side: req.side.clone(),
            action: req.action.clone(),
            yes_price,
            no_price,
            count_fp: Some(req.count_fp),
            remaining_count_fp: Some(0.0),
            count: Some(count),
            remaining_count: Some(0),
            created_time: Some(now.clone()),
        };

        let fill = Fill {
            trade_id,
            order_id: order_id.clone(),
            ticker: req.ticker.clone(),
            side: req.side.clone(),
            action: req.action.clone(),
            yes_price,
            count,
            is_taker: true,
            created_time: now,
        };

        // Deduct cost (buy) or credit proceeds (sell), in cents.
        let cost = match (req.action.as_str(), req.side.as_str()) {
            ("buy", "yes") => yes_price * count,
            ("buy", "no") => no_price * count,
            ("sell", "yes") => -(yes_price * count),
            ("sell", "no") => -(no_price * count),
            _ => 0,
        };
        self.balance -= cost;

        self.orders.insert(order_id, order.clone());
        self.fills.push(fill);

        order
    }

    /// Cancel a resting order. Returns None if not found or already executed.
    pub fn cancel_order(&mut self, order_id: &str) -> Option<Order> {
        let order = self.orders.get_mut(order_id)?;
        if order.status == "resting" {
            order.status = "cancelled".to_string();
            Some(order.clone())
        } else {
            None
        }
    }

    /// Cancel all resting orders. Returns count cancelled.
    pub fn cancel_all(&mut self) -> i32 {
        let mut count = 0;
        for order in self.orders.values_mut() {
            if order.status == "resting" {
                order.status = "cancelled".to_string();
                count += 1;
            }
        }
        count
    }

    /// Compute positions from fill history.
    pub fn get_positions(&self) -> Vec<Position> {
        let mut positions: HashMap<String, Position> = HashMap::new();
        for fill in &self.fills {
            let pos = positions
                .entry(fill.ticker.clone())
                .or_insert_with(|| Position {
                    ticker: fill.ticker.clone(),
                    position: 0,
                    market_exposure: 0,
                    total_traded: 0,
                    realized_pnl: 0,
                    resting_orders_count: Some(0),
                });

            let qty = fill.count;
            match (fill.action.as_str(), fill.side.as_str()) {
                ("buy", "yes") => pos.position += qty,
                ("sell", "yes") => pos.position -= qty,
                ("buy", "no") => pos.position -= qty,
                ("sell", "no") => pos.position += qty,
                _ => {}
            }
            pos.total_traded += qty;
        }
        positions.into_values().collect()
    }
}
