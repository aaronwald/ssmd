use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use harman::db;
use harman::error::EnqueueError;
use harman::state::OrderState;
use harman::types::{Action, Order, OrderRequest, Side, TimeInForce};

use crate::AppState;

/// Extract bearer token from Authorization header
fn extract_bearer(req: &Request) -> Option<&str> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

/// Middleware: require valid API or admin token
async fn require_api_token(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    match extract_bearer(&req) {
        Some(t)
            if bool::from(t.as_bytes().ct_eq(state.api_token.as_bytes()))
                || bool::from(t.as_bytes().ct_eq(state.admin_token.as_bytes())) =>
        {
            Ok(next.run(req).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Middleware: require valid admin token
async fn require_admin_token(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    match extract_bearer(&req) {
        Some(t) if bool::from(t.as_bytes().ct_eq(state.admin_token.as_bytes())) => {
            Ok(next.run(req).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Build the axum router with auth middleware
pub fn router(state: Arc<AppState>) -> Router {
    let public = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics));

    let api = Router::new()
        .route("/v1/orders", post(create_order))
        .route("/v1/orders", get(list_orders))
        .route("/v1/orders/:id", get(get_order))
        .route("/v1/orders/:id", delete(cancel_order))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_token,
        ));

    let admin = Router::new()
        .route("/v1/orders/mass-cancel", post(mass_cancel))
        .route("/v1/admin/pump", post(pump_handler))
        .route("/v1/admin/reconcile", post(reconcile_handler))
        .route("/v1/admin/risk", get(risk_handler))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_admin_token,
        ));

    public.merge(api).merge(admin).with_state(state)
}

/// POST /v1/orders
#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest {
    pub client_order_id: Uuid,
    pub ticker: String,
    pub side: Side,
    pub action: Action,
    #[serde(with = "rust_decimal::serde::str")]
    pub quantity: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub price_dollars: Decimal,
    #[serde(default = "default_tif")]
    pub time_in_force: TimeInForce,
}

fn default_tif() -> TimeInForce {
    TimeInForce::Gtc
}

async fn create_order(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateOrderRequest>,
) -> impl IntoResponse {
    if state.shutting_down.load(std::sync::atomic::Ordering::Relaxed) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "shutting down"})),
        )
            .into_response();
    }

    // Validate
    if req.quantity <= Decimal::ZERO {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "quantity must be positive"})),
        )
            .into_response();
    }
    if req.price_dollars <= Decimal::ZERO || req.price_dollars >= Decimal::ONE {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "price_dollars must be between 0 and 1 exclusive"})),
        )
            .into_response();
    }

    let order_req = OrderRequest {
        client_order_id: req.client_order_id,
        ticker: req.ticker,
        side: req.side,
        action: req.action,
        quantity: req.quantity,
        price_dollars: req.price_dollars,
        time_in_force: req.time_in_force,
    };

    match db::enqueue_order(&state.pool, &order_req, state.session_id, &state.risk_limits).await {
        Ok(order) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": order.id,
                "client_order_id": order.client_order_id,
                "status": "pending"
            })),
        )
            .into_response(),
        Err(EnqueueError::DuplicateClientOrderId(cid)) => {
            // Idempotency replay: if the order exists in this session and is beyond Pending,
            // return 200 with the order and X-Idempotent-Replay header.
            match db::get_order_by_client_id(&state.pool, cid, state.session_id).await {
                Ok(Some(order)) if order.state != OrderState::Pending => {
                    let mut headers = HeaderMap::new();
                    headers.insert("x-idempotent-replay", "true".parse().unwrap());
                    (StatusCode::OK, headers, Json(order_to_json(&order))).into_response()
                }
                _ => {
                    // Still pending (in-flight), not in this session, or lookup failed → 409
                    (
                        StatusCode::CONFLICT,
                        Json(serde_json::json!({"error": "duplicate client_order_id"})),
                    )
                        .into_response()
                }
            }
        }
        Err(EnqueueError::RiskCheck(e)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
        Err(EnqueueError::Database(e)) => {
            tracing::error!(error = %e, "database error creating order");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// GET /v1/orders
#[derive(Debug, Deserialize)]
pub struct ListOrdersQuery {
    pub state: Option<String>,
}

async fn list_orders(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListOrdersQuery>,
) -> impl IntoResponse {
    let state_filter = query.state.and_then(|s| match s.as_str() {
        "pending" => Some(OrderState::Pending),
        "submitted" => Some(OrderState::Submitted),
        "acknowledged" => Some(OrderState::Acknowledged),
        "partially_filled" => Some(OrderState::PartiallyFilled),
        "filled" => Some(OrderState::Filled),
        "pending_cancel" => Some(OrderState::PendingCancel),
        "cancelled" => Some(OrderState::Cancelled),
        "rejected" => Some(OrderState::Rejected),
        "expired" => Some(OrderState::Expired),
        _ => None,
    });

    match db::list_orders(&state.pool, state.session_id, state_filter).await {
        Ok(orders) => {
            let response: Vec<serde_json::Value> = orders.iter().map(order_to_json).collect();
            (StatusCode::OK, Json(serde_json::json!({"orders": response}))).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "list orders failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// GET /v1/orders/:id
async fn get_order(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match db::get_order(&state.pool, id, state.session_id).await {
        Ok(Some(order)) => (StatusCode::OK, Json(order_to_json(&order))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "order not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "get order failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// DELETE /v1/orders/:id
///
/// Atomically transitions order to PendingCancel and enqueues the cancel
/// in a single transaction to prevent inconsistent state.
async fn cancel_order(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match db::atomic_cancel_order(
        &state.pool,
        id,
        state.session_id,
        &harman::types::CancelReason::UserRequested,
    )
    .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "pending_cancel"})),
        )
            .into_response(),
        Err(e) if e.contains("not found") => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "order not found"})),
        )
            .into_response(),
        Err(e) if e.contains("cannot cancel") => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "cancel order failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// POST /v1/orders/mass-cancel
#[derive(Debug, Deserialize)]
struct MassCancelRequest {
    confirm: bool,
}

async fn mass_cancel(
    State(state): State<Arc<AppState>>,
    Json(body): Json<MassCancelRequest>,
) -> impl IntoResponse {
    if !body.confirm {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "must set confirm: true"})),
        )
            .into_response();
    }

    match state.exchange.cancel_all_orders().await {
        Ok(count) => (
            StatusCode::OK,
            Json(serde_json::json!({"cancelled": count})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "mass cancel failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

/// GET /health
async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let shutting_down = state
        .shutting_down
        .load(std::sync::atomic::Ordering::Relaxed);

    if shutting_down {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "shutting_down"})),
        )
            .into_response();
    }

    // Check DB connectivity
    match state.pool.get().await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "healthy"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "unhealthy", "error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /metrics
async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let encoder = prometheus::TextEncoder::new();
    let families = state.metrics.registry.gather();
    match encoder.encode_to_string(&families) {
        Ok(text) => (StatusCode::OK, text).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("metrics encoding error: {}", e),
        )
            .into_response(),
    }
}

/// GET /v1/admin/risk
///
/// Return current risk limits and open notional exposure.
async fn risk_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match db::compute_risk_state(&state.pool, state.session_id).await {
        Ok(risk_state) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "max_notional": state.risk_limits.max_notional.to_string(),
                "open_notional": risk_state.open_notional.to_string(),
                "available_notional": (state.risk_limits.max_notional - risk_state.open_notional).to_string(),
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "risk state query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// POST /v1/admin/pump
///
/// Drain all pending queue items, submit/cancel to exchange, return results.
async fn pump_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if state
        .shutting_down
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "shutting down"})),
        )
            .into_response();
    }

    let result = crate::pump::pump(&state).await;
    (StatusCode::OK, Json(result)).into_response()
}

/// POST /v1/admin/reconcile
///
/// Discover fills, resolve stale orders, return results.
async fn reconcile_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let result = crate::reconciliation::reconcile(&state).await;
    (StatusCode::OK, Json(result)).into_response()
}

fn order_to_json(order: &Order) -> serde_json::Value {
    serde_json::json!({
        "id": order.id,
        "client_order_id": order.client_order_id,
        "exchange_order_id": order.exchange_order_id,
        "ticker": order.ticker,
        "side": order.side,
        "action": order.action,
        "quantity": order.quantity.to_string(),
        "price_dollars": order.price_dollars.to_string(),
        "filled_quantity": order.filled_quantity.to_string(),
        "time_in_force": order.time_in_force,
        "state": order.state.to_string(),
        "cancel_reason": order.cancel_reason,
        "created_at": order.created_at.to_rfc3339(),
        "updated_at": order.updated_at.to_rfc3339(),
    })
}
