use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Extension, Json, Router,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use uuid::Uuid;

use harman::db;
use harman::error::EnqueueError;
use harman::state::OrderState;
use harman::types::{Action, GroupState, Order, OrderGroup, OrderRequest, Side, TimeInForce};

use tower_http::cors::{Any, CorsLayer};

use crate::{AppState, SessionContext};

/// Extract bearer token from Authorization header
fn extract_bearer(req: &Request) -> Option<&str> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

/// Hash a token for cache key (avoid storing raw tokens in memory)
fn hash_token(token: &str) -> u64 {
    let hash = Sha256::digest(token.as_bytes());
    u64::from_le_bytes(hash[..8].try_into().unwrap())
}

/// Check if scopes include a required scope (with hierarchy)
fn has_scope(scopes: &[String], required: &str) -> bool {
    if scopes.iter().any(|s| s == required || s == "*") {
        return true;
    }
    // harman:admin implies harman:write
    if required == "harman:write" && scopes.iter().any(|s| s == "harman:admin") {
        return true;
    }
    // harman:write implies harman:read
    if required == "harman:read" && scopes.iter().any(|s| s == "harman:write") {
        return true;
    }
    // harman:admin implies harman:read
    if required == "harman:read" && scopes.iter().any(|s| s == "harman:admin") {
        return true;
    }
    false
}

/// Require a scope from the session context, returning 403 if missing
fn require_scope(ctx: &SessionContext, scope: &str) -> Result<(), StatusCode> {
    if has_scope(&ctx.scopes, scope) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Response from data-ts /v1/auth/validate
#[derive(Deserialize)]
struct ValidateResponse {
    valid: bool,
    scopes: Vec<String>,
    key_prefix: String,
}

/// Resolve key_prefix → session_id (DashMap cache → DB)
async fn resolve_session(state: &AppState, key_prefix: &str) -> Result<i64, String> {
    if let Some(id) = state.key_sessions.get(key_prefix) {
        return Ok(*id);
    }
    let session_id =
        db::get_or_create_session(&state.pool, "kalshi", Some(key_prefix)).await?;
    state
        .key_sessions
        .insert(key_prefix.to_string(), session_id);
    Ok(session_id)
}

/// Unified auth middleware: static tokens (backward compat) + API key validation via data-ts
async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = extract_bearer(&req).ok_or(StatusCode::UNAUTHORIZED)?;

    // Path 1: Static API token (backward compat)
    if bool::from(token.as_bytes().ct_eq(state.api_token.as_bytes())) {
        req.extensions_mut().insert(SessionContext {
            session_id: state.startup_session_id,
            scopes: vec!["harman:write".into()],
            key_prefix: "static-api".into(),
        });
        return Ok(next.run(req).await);
    }

    // Path 2: Static admin token (backward compat)
    if bool::from(token.as_bytes().ct_eq(state.admin_token.as_bytes())) {
        req.extensions_mut().insert(SessionContext {
            session_id: state.startup_session_id,
            scopes: vec!["harman:admin".into()],
            key_prefix: "static-admin".into(),
        });
        return Ok(next.run(req).await);
    }

    // Path 3: API key validation via data-ts
    let auth_url = state
        .auth_validate_url
        .as_ref()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check cache (30s TTL)
    let token_hash = hash_token(token);
    {
        let cache = state.auth_cache.read().await;
        if let Some(cached) = cache.get(&token_hash) {
            if cached.cached_at.elapsed() < Duration::from_secs(30) {
                let session_id = resolve_session(&state, &cached.key_prefix)
                    .await
                    .map_err(|e| {
                        tracing::error!(error = %e, key_prefix = %cached.key_prefix, "resolve_session failed (cached)");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                req.extensions_mut().insert(SessionContext {
                    session_id,
                    scopes: cached.scopes.clone(),
                    key_prefix: cached.key_prefix.clone(),
                });
                return Ok(next.run(req).await);
            }
        }
    }

    // Cache miss — validate via HTTP
    let resp = state
        .http_client
        .get(auth_url)
        .bearer_auth(token)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "auth validation HTTP request failed");
            StatusCode::BAD_GATEWAY
        })?;

    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "auth validation rejected by data-ts");
        return Err(StatusCode::UNAUTHORIZED);
    }

    let body: ValidateResponse = resp.json().await.map_err(|e| {
        tracing::error!(error = %e, "auth validation response parse failed");
        StatusCode::BAD_GATEWAY
    })?;

    if !body.valid {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Cache result
    {
        let mut cache = state.auth_cache.write().await;
        cache.insert(
            token_hash,
            crate::CachedAuth {
                key_prefix: body.key_prefix.clone(),
                scopes: body.scopes.clone(),
                cached_at: std::time::Instant::now(),
            },
        );
    }

    let session_id = resolve_session(&state, &body.key_prefix)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, key_prefix = %body.key_prefix, "resolve_session failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    req.extensions_mut().insert(SessionContext {
        session_id,
        scopes: body.scopes,
        key_prefix: body.key_prefix,
    });

    Ok(next.run(req).await)
}

/// Build the axum router with unified auth middleware
pub fn router(state: Arc<AppState>) -> Router {
    let public = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics));

    let authenticated = Router::new()
        // harman:write
        .route("/v1/orders", post(create_order))
        .route("/v1/orders/:id", delete(cancel_order))
        .route("/v1/orders/:id/amend", post(amend_order))
        .route("/v1/orders/:id/decrease", post(decrease_order))
        .route("/v1/groups/bracket", post(create_bracket_group))
        .route("/v1/groups/oco", post(create_oco_group))
        .route("/v1/groups/:id", delete(cancel_group_handler))
        // harman:read
        .route("/v1/orders", get(list_orders))
        .route("/v1/orders/:id", get(get_order))
        .route("/v1/groups", get(list_groups_handler))
        .route("/v1/groups/:id", get(get_group_handler))
        .route("/v1/fills", get(list_fills_handler))
        .route("/v1/audit", get(list_audit_handler))
        .route("/v1/tickers", get(list_tickers_handler))
        .route("/v1/snap", get(snap_handler))
        // harman:read (monitor)
        .route("/v1/monitor/categories", get(monitor_categories_handler))
        .route("/v1/monitor/series", get(monitor_series_handler))
        .route("/v1/monitor/events", get(monitor_events_handler))
        .route("/v1/monitor/markets", get(monitor_markets_handler))
        // harman:admin
        .route("/v1/orders/mass-cancel", post(mass_cancel))
        .route("/v1/admin/pump", post(pump_handler))
        .route("/v1/admin/reconcile", post(reconcile_handler))
        .route("/v1/admin/resume", post(resume_handler))
        .route("/v1/admin/positions", get(positions_handler))
        .route("/v1/admin/risk", get(risk_handler))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    public.merge(authenticated).layer(cors).with_state(state)
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
    Extension(ctx): Extension<SessionContext>,
    Json(req): Json<CreateOrderRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:write") {
        return e.into_response();
    }

    if state.ems.is_shutting_down() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "shutting down"})),
        )
            .into_response();
    }

    if state.oms.is_suspended(ctx.session_id) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "session suspended"})),
        )
            .into_response();
    }

    // Validate
    if req.ticker.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "ticker is required"})),
        )
            .into_response();
    }
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

    match state.ems.enqueue(ctx.session_id, &order_req).await {
        Ok(order) => {
            if state.auto_pump {
                state.pump_trigger.notify(ctx.session_id);
            }
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "id": order.id,
                    "client_order_id": order.client_order_id,
                    "status": "pending"
                })),
            )
                .into_response()
        }
        Err(EnqueueError::DuplicateClientOrderId(cid)) => {
            match db::get_order_by_client_id(&state.pool, cid, ctx.session_id).await {
                Ok(Some(order)) if order.state != OrderState::Pending => {
                    let mut headers = HeaderMap::new();
                    headers.insert("x-idempotent-replay", "true".parse().unwrap());
                    (StatusCode::OK, headers, Json(order_to_json(&order))).into_response()
                }
                _ => (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"error": "duplicate client_order_id"})),
                )
                    .into_response(),
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
    Extension(ctx): Extension<SessionContext>,
    Query(query): Query<ListOrdersQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        return e.into_response();
    }

    // Support individual states plus "open" and "terminal" group filters
    let group_filter = query.state.clone();
    let state_filter = query.state.and_then(|s| match s.as_str() {
        "pending" => Some(OrderState::Pending),
        "submitted" => Some(OrderState::Submitted),
        "acknowledged" => Some(OrderState::Acknowledged),
        "partially_filled" => Some(OrderState::PartiallyFilled),
        "filled" => Some(OrderState::Filled),
        "pending_cancel" => Some(OrderState::PendingCancel),
        "pending_amend" => Some(OrderState::PendingAmend),
        "pending_decrease" => Some(OrderState::PendingDecrease),
        "cancelled" => Some(OrderState::Cancelled),
        "rejected" => Some(OrderState::Rejected),
        "expired" => Some(OrderState::Expired),
        "staged" => Some(OrderState::Staged),
        _ => None,
    });

    match db::list_orders(&state.pool, ctx.session_id, state_filter).await {
        Ok(orders) => {
            let filtered: Vec<_> = match group_filter.as_deref() {
                Some("open") => orders.into_iter().filter(|o| o.state.is_open()).collect(),
                Some("terminal") => orders.into_iter().filter(|o| o.state.is_terminal()).collect(),
                Some("resting") => orders.into_iter()
                    .filter(|o| matches!(o.state, OrderState::Acknowledged | OrderState::PartiallyFilled))
                    .collect(),
                Some("today") => {
                    let today = chrono::Utc::now().date_naive();
                    orders.into_iter()
                        .filter(|o| o.created_at.date_naive() == today)
                        .collect()
                }
                _ => orders,
            };
            let response: Vec<serde_json::Value> = filtered.iter().map(order_to_json).collect();
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
    Extension(ctx): Extension<SessionContext>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        return e.into_response();
    }

    match db::get_order(&state.pool, id, ctx.session_id).await {
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
async fn cancel_order(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:write") {
        return e.into_response();
    }

    match state
        .ems
        .enqueue_cancel(id, ctx.session_id, &harman::types::CancelReason::UserRequested)
        .await
    {
        Ok(()) => {
            if state.auto_pump {
                state.pump_trigger.notify(ctx.session_id);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "pending_cancel"})),
            )
                .into_response()
        }
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

/// POST /v1/orders/:id/amend
#[derive(Debug, Deserialize)]
pub struct AmendOrderRequest {
    #[serde(default)]
    pub new_price_dollars: Option<String>,
    #[serde(default)]
    pub new_quantity: Option<String>,
}

async fn amend_order(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Path(id): Path<i64>,
    Json(body): Json<AmendOrderRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:write") {
        return e.into_response();
    }

    // At least one field required
    if body.new_price_dollars.is_none() && body.new_quantity.is_none() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "at least one of new_price_dollars or new_quantity required"})),
        )
            .into_response();
    }

    let new_price: Option<Decimal> = match &body.new_price_dollars {
        Some(s) => match s.parse::<Decimal>() {
            Ok(d) if d > Decimal::ZERO && d < Decimal::ONE => Some(d),
            Ok(_) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({"error": "new_price_dollars must be between 0 and 1 exclusive"})),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({"error": "invalid new_price_dollars"})),
                )
                    .into_response();
            }
        },
        None => None,
    };

    let new_qty: Option<Decimal> = match &body.new_quantity {
        Some(s) => match s.parse::<Decimal>() {
            Ok(d) if d > Decimal::ZERO => Some(d),
            Ok(_) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({"error": "new_quantity must be positive"})),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({"error": "invalid new_quantity"})),
                )
                    .into_response();
            }
        },
        None => None,
    };

    match state.ems.enqueue_amend(id, ctx.session_id, new_price, new_qty).await {
        Ok(()) => {
            if state.auto_pump {
                state.pump_trigger.notify(ctx.session_id);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "pending_amend"})),
            )
                .into_response()
        }
        Err(e) if e.contains("not found") => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "order not found"})),
        )
            .into_response(),
        Err(e) if e.contains("cannot amend") => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "amend order failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// POST /v1/orders/:id/decrease
#[derive(Debug, Deserialize)]
pub struct DecreaseOrderRequest {
    pub reduce_by: String,
}

async fn decrease_order(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Path(id): Path<i64>,
    Json(body): Json<DecreaseOrderRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:write") {
        return e.into_response();
    }

    let reduce_by = match body.reduce_by.parse::<Decimal>() {
        Ok(d) if d > Decimal::ZERO => d,
        Ok(_) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "reduce_by must be positive"})),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "invalid reduce_by"})),
            )
                .into_response();
        }
    };

    match state.ems.enqueue_decrease(id, ctx.session_id, reduce_by).await {
        Ok(()) => {
            if state.auto_pump {
                state.pump_trigger.notify(ctx.session_id);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "pending_decrease"})),
            )
                .into_response()
        }
        Err(e) if e.contains("not found") => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "order not found"})),
        )
            .into_response(),
        Err(e) if e.contains("cannot decrease") || e.contains("reduce_by") => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "decrease order failed");
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
    Extension(ctx): Extension<SessionContext>,
    Json(body): Json<MassCancelRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:admin") {
        return e.into_response();
    }

    if !body.confirm {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "must set confirm: true"})),
        )
            .into_response();
    }

    match state.ems.exchange.cancel_all_orders().await {
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
    if state.ems.is_shutting_down() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "shutting_down"})),
        )
            .into_response();
    }

    let any_suspended = !state.oms.suspended_sessions.is_empty();

    // Check DB connectivity
    match state.pool.get().await {
        Ok(_) => {
            let status = if any_suspended { "suspended" } else { "healthy" };
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": status, "suspended": any_suspended})),
            )
                .into_response()
        }
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
    let families = state.registry.gather();
    match encoder.encode_to_string(&families) {
        Ok(text) => (StatusCode::OK, text).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("metrics encoding error: {}", e),
        )
            .into_response(),
    }
}

/// GET /v1/admin/positions
///
/// Returns both exchange positions (from Kalshi API) and local positions
/// (computed from filled orders in DB). This lets the user compare both
/// views and spot discrepancies.
async fn positions_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        return e.into_response();
    }

    match state.oms.positions(ctx.session_id).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "positions failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// GET /v1/admin/risk
async fn risk_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:admin") {
        return e.into_response();
    }

    match db::compute_risk_state(&state.pool, ctx.session_id).await {
        Ok(risk_state) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "max_notional": state.ems.risk_limits.max_notional.to_string(),
                "open_notional": risk_state.open_notional.to_string(),
                "available_notional": (state.ems.risk_limits.max_notional - risk_state.open_notional).to_string(),
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
async fn pump_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:admin") {
        return e.into_response();
    }

    if state.ems.is_shutting_down() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "shutting down"})),
        )
            .into_response();
    }

    if state.oms.is_suspended(ctx.session_id) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "session suspended"})),
        )
            .into_response();
    }

    // Per-session pump semaphore
    let sem = state
        .session_semaphores
        .entry(ctx.session_id)
        .or_insert_with(|| Arc::new(Semaphore::new(1)))
        .clone();
    let _permit = match sem.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "pump already running"})),
            )
                .into_response();
        }
    };

    let result = crate::pump::pump(&state, ctx.session_id).await;
    (StatusCode::OK, Json(result)).into_response()
}

/// POST /v1/admin/reconcile
async fn reconcile_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:admin") {
        return e.into_response();
    }

    let result = state.oms.reconcile(ctx.session_id).await;
    (StatusCode::OK, Json(result)).into_response()
}

/// POST /v1/admin/resume
async fn resume_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:admin") {
        return e.into_response();
    }

    let was_suspended = state.oms.resume(ctx.session_id);
    tracing::info!(session_id = ctx.session_id, was_suspended, "admin resumed session");
    (
        StatusCode::OK,
        Json(serde_json::json!({"resumed": true, "was_suspended": was_suspended})),
    )
        .into_response()
}

/// GET /v1/fills
#[derive(Debug, Deserialize)]
pub struct ListFillsQuery {
    pub limit: Option<i64>,
}

async fn list_fills_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Query(query): Query<ListFillsQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        return e.into_response();
    }

    let limit = query.limit.unwrap_or(100).min(1000);

    match db::list_fills(&state.pool, ctx.session_id, limit).await {
        Ok(fills) => (StatusCode::OK, Json(serde_json::json!({"fills": fills}))).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "list fills failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// GET /v1/audit
#[derive(Debug, Deserialize)]
pub struct ListAuditQuery {
    pub limit: Option<i64>,
}

async fn list_audit_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Query(query): Query<ListAuditQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        return e.into_response();
    }

    let limit = query.limit.unwrap_or(100).min(1000);

    match db::list_audit_log(&state.pool, ctx.session_id, limit).await {
        Ok(entries) => {
            (StatusCode::OK, Json(serde_json::json!({"audit": entries}))).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "list audit log failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// GET /v1/tickers — proxy to data-ts secmaster for ticker autocomplete.
/// Caches for 5 minutes to avoid hammering data-ts.
async fn list_tickers_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    headers: HeaderMap,
    Query(query): Query<TickerSearchQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        return e.into_response();
    }

    let prefix = query.q.as_deref().unwrap_or("");

    // Check cache (5 minute TTL)
    {
        let cache = state.ticker_cache.read().await;
        if let Some((cached_at, tickers)) = cache.as_ref() {
            if cached_at.elapsed() < Duration::from_secs(300) {
                let filtered = filter_tickers(tickers, prefix, 50);
                return (StatusCode::OK, Json(serde_json::json!({"tickers": filtered}))).into_response();
            }
        }
    }

    // Cache miss — fetch from data-ts
    let base_url = match &state.auth_validate_url {
        Some(url) => url.replace("/v1/auth/validate", ""),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "data-ts not configured"})),
            ).into_response();
        }
    };

    let url = format!("{}/v1/markets?status=active&limit=500", base_url);
    let mut req = state.http_client.get(&url).timeout(Duration::from_secs(10));
    // Use DATA_TS_API_KEY if configured, otherwise try forwarding user's auth
    if let Ok(key) = std::env::var("DATA_TS_API_KEY") {
        req = req.header("authorization", format!("Bearer {}", key));
    } else if let Some(auth) = headers.get("authorization") {
        if let Ok(val) = auth.to_str() {
            req = req.header("authorization", val);
        }
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "failed to fetch tickers from data-ts, returning empty");
            let filtered = filter_tickers(&[], prefix, 50);
            return (StatusCode::OK, Json(serde_json::json!({"tickers": filtered}))).into_response();
        }
    };

    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "data-ts returned error for markets, returning empty");
        let filtered = filter_tickers(&[], prefix, 50);
        return (StatusCode::OK, Json(serde_json::json!({"tickers": filtered}))).into_response();
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "failed to parse data-ts markets response");
            return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": "parse error"}))).into_response();
        }
    };

    // Extract ticker strings from the markets array
    let tickers: Vec<String> = body["markets"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["ticker"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Update cache
    {
        let mut cache = state.ticker_cache.write().await;
        *cache = Some((std::time::Instant::now(), tickers.clone()));
    }

    let filtered = filter_tickers(&tickers, prefix, 50);
    (StatusCode::OK, Json(serde_json::json!({"tickers": filtered}))).into_response()
}

/// Filter tickers by prefix (case-insensitive) and limit results
fn filter_tickers<'a>(tickers: &'a [String], prefix: &str, limit: usize) -> Vec<&'a str> {
    if prefix.is_empty() {
        return tickers.iter().map(|s| s.as_str()).take(limit).collect();
    }
    let prefix_upper = prefix.to_uppercase();
    tickers
        .iter()
        .filter(|t| t.to_uppercase().starts_with(&prefix_upper))
        .map(|s| s.as_str())
        .take(limit)
        .collect()
}

#[derive(Debug, Deserialize)]
struct TickerSearchQuery {
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SnapQuery {
    feed: Option<String>,
    tickers: Option<String>,
}

/// GET /v1/snap — proxy to data-ts snap endpoint for live market data.
/// Caches for 30 seconds per feed.
async fn snap_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Query(query): Query<SnapQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        return e.into_response();
    }

    let feed = query.feed.as_deref().unwrap_or("kalshi");

    let base_url = match &state.auth_validate_url {
        Some(url) => url.replace("/v1/auth/validate", ""),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "data-ts not configured"})),
            )
                .into_response();
        }
    };

    let mut url = format!("{}/v1/data/snap?feed={}", base_url, feed);
    if let Some(tickers) = &query.tickers {
        url.push_str(&format!("&tickers={}", tickers));
    }

    let mut req = state.http_client.get(&url).timeout(Duration::from_secs(10));
    if let Ok(key) = std::env::var("DATA_TS_API_KEY") {
        req = req.header("authorization", format!("Bearer {}", key));
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "failed to fetch snap from data-ts");
            return (
                StatusCode::OK,
                Json(serde_json::json!({"feed": feed, "snapshots": [], "count": 0})),
            )
                .into_response();
        }
    };

    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "data-ts snap returned error");
        return (
            StatusCode::OK,
            Json(serde_json::json!({"feed": feed, "snapshots": [], "count": 0})),
        )
            .into_response();
    }

    match resp.json::<serde_json::Value>().await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "failed to parse data-ts snap response");
            (
                StatusCode::OK,
                Json(serde_json::json!({"feed": feed, "snapshots": [], "count": 0})),
            )
                .into_response()
        }
    }
}

// --- Monitor endpoints (Redis-backed, from ssmd-cache) ---

#[derive(Debug, Deserialize)]
struct MonitorQuery {
    category: Option<String>,
    series: Option<String>,
    event: Option<String>,
}

/// GET /v1/monitor/categories — list all categories with event/series counts
async fn monitor_categories_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        state.monitor_metrics.requests_total.with_label_values(&["categories", "forbidden"]).inc();
        return e.into_response();
    }
    let Some(ref conn) = state.redis_conn else {
        state.monitor_metrics.requests_total.with_label_values(&["categories", "ok"]).inc();
        return (StatusCode::OK, Json(serde_json::json!({"categories": []}))).into_response();
    };
    let mut conn = conn.clone();
    let timer = state.monitor_metrics.redis_duration_seconds.start_timer();
    let result: std::collections::HashMap<String, String> = match redis::cmd("HGETALL")
        .arg("monitor:categories")
        .query_async(&mut conn)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            timer.observe_duration();
            state.monitor_metrics.redis_errors_total.inc();
            tracing::warn!(error = %e, "Redis HGETALL monitor:categories failed");
            state.monitor_metrics.requests_total.with_label_values(&["categories", "ok"]).inc();
            return (StatusCode::OK, Json(serde_json::json!({"categories": []}))).into_response();
        }
    };
    timer.observe_duration();
    let categories: Vec<serde_json::Value> = result
        .into_iter()
        .map(|(name, val)| {
            let mut obj: serde_json::Value =
                serde_json::from_str(&val).unwrap_or(serde_json::json!({}));
            obj["name"] = serde_json::json!(name);
            obj
        })
        .collect();
    state.monitor_metrics.requests_total.with_label_values(&["categories", "ok"]).inc();
    (StatusCode::OK, Json(serde_json::json!({"categories": categories}))).into_response()
}

/// GET /v1/monitor/series?category=Crypto — list series in a category
async fn monitor_series_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Query(query): Query<MonitorQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        state.monitor_metrics.requests_total.with_label_values(&["series", "forbidden"]).inc();
        return e.into_response();
    }
    let Some(category) = &query.category else {
        state.monitor_metrics.requests_total.with_label_values(&["series", "bad_request"]).inc();
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "category query parameter is required"})),
        )
            .into_response();
    };
    let Some(ref conn) = state.redis_conn else {
        state.monitor_metrics.requests_total.with_label_values(&["series", "ok"]).inc();
        return (StatusCode::OK, Json(serde_json::json!({"series": []}))).into_response();
    };
    let mut conn = conn.clone();
    let key = format!("monitor:series:{}", category);
    let timer = state.monitor_metrics.redis_duration_seconds.start_timer();
    let result: std::collections::HashMap<String, String> = match redis::cmd("HGETALL")
        .arg(&key)
        .query_async(&mut conn)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            timer.observe_duration();
            state.monitor_metrics.redis_errors_total.inc();
            tracing::warn!(error = %e, key, "Redis HGETALL failed");
            state.monitor_metrics.requests_total.with_label_values(&["series", "ok"]).inc();
            return (StatusCode::OK, Json(serde_json::json!({"series": []}))).into_response();
        }
    };
    timer.observe_duration();
    let series: Vec<serde_json::Value> = result
        .into_iter()
        .map(|(ticker, val)| {
            let mut obj: serde_json::Value =
                serde_json::from_str(&val).unwrap_or(serde_json::json!({}));
            obj["ticker"] = serde_json::json!(ticker);
            obj
        })
        .collect();
    state.monitor_metrics.requests_total.with_label_values(&["series", "ok"]).inc();
    (StatusCode::OK, Json(serde_json::json!({"series": series}))).into_response()
}

/// GET /v1/monitor/events?series=KXBTCD — list events in a series
async fn monitor_events_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Query(query): Query<MonitorQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        state.monitor_metrics.requests_total.with_label_values(&["events", "forbidden"]).inc();
        return e.into_response();
    }
    let Some(series) = &query.series else {
        state.monitor_metrics.requests_total.with_label_values(&["events", "bad_request"]).inc();
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "series query parameter is required"})),
        )
            .into_response();
    };
    let Some(ref conn) = state.redis_conn else {
        state.monitor_metrics.requests_total.with_label_values(&["events", "ok"]).inc();
        return (StatusCode::OK, Json(serde_json::json!({"events": []}))).into_response();
    };
    let mut conn = conn.clone();
    let key = format!("monitor:events:{}", series);
    let timer = state.monitor_metrics.redis_duration_seconds.start_timer();
    let result: std::collections::HashMap<String, String> = match redis::cmd("HGETALL")
        .arg(&key)
        .query_async(&mut conn)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            timer.observe_duration();
            state.monitor_metrics.redis_errors_total.inc();
            tracing::warn!(error = %e, key, "Redis HGETALL failed");
            state.monitor_metrics.requests_total.with_label_values(&["events", "ok"]).inc();
            return (StatusCode::OK, Json(serde_json::json!({"events": []}))).into_response();
        }
    };
    timer.observe_duration();
    let events: Vec<serde_json::Value> = result
        .into_iter()
        .map(|(ticker, val)| {
            let mut obj: serde_json::Value =
                serde_json::from_str(&val).unwrap_or(serde_json::json!({}));
            obj["ticker"] = serde_json::json!(ticker);
            obj
        })
        .collect();
    state.monitor_metrics.requests_total.with_label_values(&["events", "ok"]).inc();
    (StatusCode::OK, Json(serde_json::json!({"events": events}))).into_response()
}

/// GET /v1/monitor/markets?event=KXBTCD-26FEB28 — list markets with live snap data
async fn monitor_markets_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Query(query): Query<MonitorQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        state.monitor_metrics.requests_total.with_label_values(&["markets", "forbidden"]).inc();
        return e.into_response();
    }
    let Some(event) = &query.event else {
        state.monitor_metrics.requests_total.with_label_values(&["markets", "bad_request"]).inc();
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "event query parameter is required"})),
        )
            .into_response();
    };
    let Some(ref conn) = state.redis_conn else {
        state.monitor_metrics.requests_total.with_label_values(&["markets", "ok"]).inc();
        return (StatusCode::OK, Json(serde_json::json!({"markets": []}))).into_response();
    };
    let mut conn = conn.clone();
    let key = format!("monitor:markets:{}", event);
    let timer = state.monitor_metrics.redis_duration_seconds.start_timer();
    let result: std::collections::HashMap<String, String> = match redis::cmd("HGETALL")
        .arg(&key)
        .query_async(&mut conn)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            timer.observe_duration();
            state.monitor_metrics.redis_errors_total.inc();
            tracing::warn!(error = %e, key, "Redis HGETALL failed");
            state.monitor_metrics.requests_total.with_label_values(&["markets", "ok"]).inc();
            return (StatusCode::OK, Json(serde_json::json!({"markets": []}))).into_response();
        }
    };
    timer.observe_duration();

    // Collect tickers and parse market objects
    let mut markets: Vec<(String, serde_json::Value)> = result
        .into_iter()
        .map(|(ticker, val)| {
            let mut obj: serde_json::Value =
                serde_json::from_str(&val).unwrap_or(serde_json::json!({}));
            obj["ticker"] = serde_json::json!(&ticker);
            (ticker, obj)
        })
        .collect();

    // Enrich with live snap data if available
    if !markets.is_empty() {
        let snap_keys: Vec<String> = markets
            .iter()
            .map(|(ticker, _)| format!("snap:kalshi:{}", ticker))
            .collect();
        let snap_timer = state.monitor_metrics.redis_duration_seconds.start_timer();
        let snap_results: Vec<Option<String>> = match redis::cmd("MGET")
            .arg(&snap_keys)
            .query_async(&mut conn)
            .await
        {
            Ok(r) => {
                snap_timer.observe_duration();
                r
            }
            Err(e) => {
                snap_timer.observe_duration();
                state.monitor_metrics.redis_errors_total.inc();
                tracing::warn!(error = %e, "Redis MGET snap keys failed");
                vec![None; markets.len()]
            }
        };

        for (i, snap_str) in snap_results.into_iter().enumerate() {
            if let Some(s) = snap_str {
                if let Ok(snap) = serde_json::from_str::<serde_json::Value>(&s) {
                    // Snap data is nested: {"type":"ticker","msg":{...prices...}}
                    let msg = snap.get("msg").unwrap_or(&snap);
                    let market = &mut markets[i].1;
                    // Convert Kalshi prices from cents to dollars
                    if let Some(yb) = msg.get("yes_bid").and_then(|v| v.as_f64()) {
                        market["yes_bid"] = serde_json::json!(yb / 100.0);
                    }
                    if let Some(ya) = msg.get("yes_ask").and_then(|v| v.as_f64()) {
                        market["yes_ask"] = serde_json::json!(ya / 100.0);
                    }
                    if let Some(lp) = msg.get("last_price").or_else(|| msg.get("price")).and_then(|v| v.as_f64()) {
                        market["last"] = serde_json::json!(lp / 100.0);
                    }
                    if let Some(vol) = msg.get("volume") {
                        market["volume"] = vol.clone();
                    }
                    if let Some(oi) = msg.get("open_interest") {
                        market["open_interest"] = oi.clone();
                    }
                }
            }
        }
    }

    let market_values: Vec<serde_json::Value> = markets.into_iter().map(|(_, v)| v).collect();
    state.monitor_metrics.requests_total.with_label_values(&["markets", "ok"]).inc();
    (StatusCode::OK, Json(serde_json::json!({"markets": market_values}))).into_response()
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
        "group_id": order.group_id,
        "leg_role": order.leg_role.map(|r| r.to_string()),
        "created_at": order.created_at.to_rfc3339(),
        "updated_at": order.updated_at.to_rfc3339(),
    })
}

fn group_to_json(group: &OrderGroup, orders: &[Order]) -> serde_json::Value {
    serde_json::json!({
        "id": group.id,
        "session_id": group.session_id,
        "group_type": group.group_type.to_string(),
        "state": group.state.to_string(),
        "orders": orders.iter().map(order_to_json).collect::<Vec<_>>(),
        "created_at": group.created_at.to_rfc3339(),
        "updated_at": group.updated_at.to_rfc3339(),
    })
}

/// POST /v1/groups/bracket
#[derive(Debug, Deserialize)]
pub struct CreateBracketRequest {
    pub entry: CreateOrderRequest,
    pub take_profit: CreateOrderRequest,
    pub stop_loss: CreateOrderRequest,
}

async fn create_bracket_group(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Json(req): Json<CreateBracketRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:write") {
        return e.into_response();
    }

    if state.ems.is_shutting_down() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "shutting down"})),
        )
            .into_response();
    }

    if state.oms.is_suspended(ctx.session_id) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "session suspended"})),
        )
            .into_response();
    }

    let entry = to_order_request(&req.entry);
    let tp = to_order_request(&req.take_profit);
    let sl = to_order_request(&req.stop_loss);

    match state.oms.create_bracket(ctx.session_id, entry, tp, sl).await {
        Ok((group, orders)) => {
            if state.auto_pump {
                state.pump_trigger.notify(ctx.session_id);
            }
            (
                StatusCode::CREATED,
                Json(group_to_json(&group, &orders)),
            )
                .into_response()
        }
        Err(EnqueueError::DuplicateClientOrderId(_)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "duplicate client_order_id"})),
        )
            .into_response(),
        Err(EnqueueError::RiskCheck(e)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
        Err(EnqueueError::Database(e)) => {
            tracing::error!(error = %e, "database error creating bracket group");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// POST /v1/groups/oco
#[derive(Debug, Deserialize)]
pub struct CreateOcoRequest {
    pub leg1: CreateOrderRequest,
    pub leg2: CreateOrderRequest,
}

async fn create_oco_group(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Json(req): Json<CreateOcoRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:write") {
        return e.into_response();
    }

    if state.ems.is_shutting_down() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "shutting down"})),
        )
            .into_response();
    }

    if state.oms.is_suspended(ctx.session_id) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "session suspended"})),
        )
            .into_response();
    }

    let leg1 = to_order_request(&req.leg1);
    let leg2 = to_order_request(&req.leg2);

    match state.oms.create_oco(ctx.session_id, leg1, leg2).await {
        Ok((group, orders)) => {
            if state.auto_pump {
                state.pump_trigger.notify(ctx.session_id);
            }
            (
                StatusCode::CREATED,
                Json(group_to_json(&group, &orders)),
            )
                .into_response()
        }
        Err(EnqueueError::DuplicateClientOrderId(_)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "duplicate client_order_id"})),
        )
            .into_response(),
        Err(EnqueueError::RiskCheck(e)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
        Err(EnqueueError::Database(e)) => {
            tracing::error!(error = %e, "database error creating OCO group");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// GET /v1/groups
#[derive(Debug, Deserialize)]
pub struct ListGroupsQuery {
    pub state: Option<String>,
}

async fn list_groups_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Query(query): Query<ListGroupsQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        return e.into_response();
    }

    let state_filter = query.state.and_then(|s| match s.as_str() {
        "active" => Some(GroupState::Active),
        "completed" => Some(GroupState::Completed),
        "cancelled" => Some(GroupState::Cancelled),
        _ => None,
    });

    match db::list_groups(&state.pool, ctx.session_id, state_filter).await {
        Ok(groups) => {
            let mut response: Vec<serde_json::Value> = Vec::with_capacity(groups.len());
            for g in &groups {
                let orders = db::get_group_orders(&state.pool, g.id, ctx.session_id)
                    .await
                    .unwrap_or_default();
                response.push(group_to_json(g, &orders));
            }
            (StatusCode::OK, Json(serde_json::json!({"groups": response}))).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "list groups failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// GET /v1/groups/:id
async fn get_group_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:read") {
        return e.into_response();
    }

    match db::get_group(&state.pool, id, ctx.session_id).await {
        Ok(Some(group)) => {
            match db::get_group_orders(&state.pool, id, ctx.session_id).await {
                Ok(orders) => {
                    (StatusCode::OK, Json(group_to_json(&group, &orders))).into_response()
                }
                Err(e) => {
                    tracing::error!(error = %e, "get group orders failed");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "internal error"})),
                    )
                        .into_response()
                }
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "group not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "get group failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// DELETE /v1/groups/:id
async fn cancel_group_handler(
    State(state): State<Arc<AppState>>,
    Extension(ctx): Extension<SessionContext>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(e) = require_scope(&ctx, "harman:write") {
        return e.into_response();
    }

    match state.oms.cancel_group(id, ctx.session_id).await {
        Ok(()) => {
            if state.auto_pump {
                state.pump_trigger.notify(ctx.session_id);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "cancelled"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "cancel group failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e})),
            )
                .into_response()
        }
    }
}

fn to_order_request(req: &CreateOrderRequest) -> OrderRequest {
    OrderRequest {
        client_order_id: req.client_order_id,
        ticker: req.ticker.clone(),
        side: req.side,
        action: req.action,
        quantity: req.quantity,
        price_dollars: req.price_dollars,
        time_in_force: req.time_in_force,
    }
}
