use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::state::{ExchangeState, Fill, Order, OrderRequest, Position};

pub type AppState = Arc<Mutex<ExchangeState>>;

// --- Response types (match Kalshi JSON shapes) ---

#[derive(Serialize)]
pub struct OrderResponse {
    pub order: Order,
}

#[derive(Serialize)]
pub struct OrdersResponse {
    pub orders: Vec<Order>,
    pub cursor: Option<String>,
}

#[derive(Serialize)]
pub struct BatchCancelResponse {
    pub orders_cancelled: i32,
}

#[derive(Serialize)]
pub struct FillsResponse {
    pub fills: Vec<Fill>,
    pub cursor: Option<String>,
}

#[derive(Serialize)]
pub struct PositionsResponse {
    pub market_positions: Vec<Position>,
    pub cursor: Option<String>,
}

#[derive(Serialize)]
pub struct BalanceResponse {
    pub balance: i64,
    pub payout: i64,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
}

// --- Query params ---

#[derive(Deserialize)]
pub struct OrdersQuery {
    pub client_order_id: Option<String>,
    pub ticker: Option<String>,
}

// --- Handlers ---

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
    })
}

pub async fn submit_order(
    State(state): State<AppState>,
    Json(req): Json<OrderRequest>,
) -> (StatusCode, Json<OrderResponse>) {
    let mut state = state.lock().await;
    tracing::info!(
        ticker = %req.ticker,
        side = %req.side,
        action = %req.action,
        count_fp = req.count_fp,
        yes_price = req.yes_price,
        client_order_id = %req.client_order_id,
        "order submitted — immediate fill"
    );
    let order = state.submit_order(&req);
    (StatusCode::OK, Json(OrderResponse { order }))
}

pub async fn cancel_order(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
) -> Result<Json<OrderResponse>, StatusCode> {
    let mut state = state.lock().await;
    match state.cancel_order(&order_id) {
        Some(order) => Ok(Json(OrderResponse { order })),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn batch_cancel(State(state): State<AppState>) -> Json<BatchCancelResponse> {
    let mut state = state.lock().await;
    let count = state.cancel_all();
    tracing::info!(cancelled = count, "batch cancel");
    Json(BatchCancelResponse {
        orders_cancelled: count,
    })
}

pub async fn list_orders(
    State(state): State<AppState>,
    Query(query): Query<OrdersQuery>,
) -> Json<OrdersResponse> {
    let state = state.lock().await;
    let mut orders: Vec<Order> = state.orders.values().cloned().collect();

    if let Some(coid) = &query.client_order_id {
        orders.retain(|o| o.client_order_id.as_deref() == Some(coid));
    }
    if let Some(ticker) = &query.ticker {
        orders.retain(|o| o.ticker == *ticker);
    }

    Json(OrdersResponse {
        orders,
        cursor: None,
    })
}

pub async fn list_fills(State(state): State<AppState>) -> Json<FillsResponse> {
    let state = state.lock().await;
    Json(FillsResponse {
        fills: state.fills.clone(),
        cursor: None,
    })
}

pub async fn list_positions(State(state): State<AppState>) -> Json<PositionsResponse> {
    let state = state.lock().await;
    let positions = state.get_positions();
    Json(PositionsResponse {
        market_positions: positions,
        cursor: None,
    })
}

pub async fn get_balance(State(state): State<AppState>) -> Json<BalanceResponse> {
    let state = state.lock().await;
    Json(BalanceResponse {
        balance: state.balance,
        payout: 0,
    })
}
