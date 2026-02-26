use serde::{Deserialize, Serialize};

/// Request body for placing an order on Kalshi
#[derive(Debug, Serialize)]
pub struct KalshiOrderRequest {
    pub ticker: String,
    pub client_order_id: String,
    pub side: String,
    pub action: String,
    #[serde(rename = "type")]
    pub order_type: String,
    pub count_fp: String,
    pub yes_price: i32,
    pub time_in_force: String,
    pub subaccount: i32,
}

/// Response from placing an order
#[derive(Debug, Deserialize)]
pub struct KalshiOrderResponse {
    pub order: KalshiOrder,
}

/// Kalshi order object
#[derive(Debug, Clone, Deserialize)]
pub struct KalshiOrder {
    pub order_id: String,
    pub client_order_id: Option<String>,
    pub ticker: String,
    pub status: String,
    pub side: String,
    pub action: String,
    #[serde(default)]
    pub yes_price: i64,
    #[serde(default)]
    pub no_price: i64,
    #[serde(default)]
    pub count_fp: Option<String>,
    #[serde(default)]
    pub remaining_count_fp: Option<String>,
    #[serde(default)]
    pub count: Option<i64>,
    #[serde(default)]
    pub remaining_count: Option<i64>,
    pub created_time: Option<String>,
}

impl KalshiOrder {
    /// Parse count_fp string to f64
    fn count_fp_as_f64(&self) -> Option<f64> {
        self.count_fp.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse remaining_count_fp string to f64
    fn remaining_count_fp_as_f64(&self) -> Option<f64> {
        self.remaining_count_fp.as_deref().and_then(|s| s.parse().ok())
    }

    /// Get the count (quantity) preferring count_fp.
    pub fn effective_count(&self) -> i32 {
        if let Some(fp) = self.count_fp_as_f64() {
            fp.round() as i32
        } else {
            self.count.unwrap_or(0) as i32
        }
    }

    /// Get the remaining count preferring remaining_count_fp.
    pub fn effective_remaining(&self) -> i32 {
        if let Some(fp) = self.remaining_count_fp_as_f64() {
            fp.round() as i32
        } else {
            self.remaining_count.unwrap_or(0) as i32
        }
    }

    /// Get filled count
    pub fn filled_count(&self) -> i32 {
        self.effective_count() - self.effective_remaining()
    }
}

/// Response from GET /portfolio/orders (list)
#[derive(Debug, Deserialize)]
pub struct KalshiOrdersResponse {
    pub orders: Vec<KalshiOrder>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Individual result from batch cancel
#[derive(Debug, Deserialize)]
pub struct KalshiBatchCancelResult {
    pub order_id: String,
    #[serde(default)]
    pub reduced_by: Option<i32>,
}

/// Response from DELETE /portfolio/orders/batched (mass cancel)
#[derive(Debug, Deserialize)]
pub struct KalshiBatchCancelResponse {
    #[serde(default)]
    pub orders: Vec<KalshiBatchCancelResult>,
}

/// Kalshi fill object
#[derive(Debug, Clone, Deserialize)]
pub struct KalshiFill {
    pub trade_id: String,
    pub order_id: String,
    pub ticker: String,
    pub side: String,
    pub action: String,
    #[serde(default)]
    pub yes_price: i64,
    #[serde(default)]
    pub count: i64,
    pub is_taker: bool,
    pub created_time: String,
}

/// Response from GET /portfolio/fills
#[derive(Debug, Deserialize)]
pub struct KalshiFillsResponse {
    pub fills: Vec<KalshiFill>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Kalshi position object
#[derive(Debug, Clone, Deserialize)]
pub struct KalshiPosition {
    pub ticker: String,
    #[serde(default)]
    pub position: i64,
    #[serde(default)]
    pub market_exposure: i64,
    #[serde(default)]
    pub total_traded: i64,
    #[serde(default)]
    pub realized_pnl: i64,
    pub resting_orders_count: Option<i64>,
}

/// Response from GET /portfolio/positions
#[derive(Debug, Deserialize)]
pub struct KalshiPositionsResponse {
    pub market_positions: Vec<KalshiPosition>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Kalshi balance object
#[derive(Debug, Deserialize)]
pub struct KalshiBalance {
    #[serde(default)]
    pub balance: i64,
    #[serde(default)]
    pub payout: i64,
}

/// Response from GET /portfolio/balance
#[derive(Debug, Deserialize)]
pub struct KalshiBalanceResponse {
    #[serde(flatten)]
    pub balance: KalshiBalance,
}

/// POST /trade-api/v2/portfolio/orders/{order_id}/amend
#[derive(Debug, Serialize)]
pub struct KalshiAmendRequest {
    pub ticker: String,
    pub side: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yes_price: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_fp: Option<String>,
    pub subaccount: i32,
}

/// Response from amend endpoint
#[derive(Debug, Deserialize)]
pub struct KalshiAmendResponse {
    pub old_order: KalshiOrder,
    pub order: KalshiOrder,
}

/// POST /trade-api/v2/portfolio/orders/{order_id}/decrease
#[derive(Debug, Serialize)]
pub struct KalshiDecreaseRequest {
    pub reduce_by_fp: String,
    pub subaccount: i32,
}

/// Kalshi API error response
#[derive(Debug, Deserialize)]
pub struct KalshiError {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}
