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
use harman::types::{Action, Order, OrderRequest, Side, TimeInForce};

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
        // harman:read
        .route("/v1/orders", get(list_orders))
        .route("/v1/orders/:id", get(get_order))
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

    public.merge(authenticated).with_state(state)
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
        _ => None,
    });

    match db::list_orders(&state.pool, ctx.session_id, state_filter).await {
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
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "pending_amend"})),
        )
            .into_response(),
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
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "pending_decrease"})),
        )
            .into_response(),
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
