//! HTTP server for health, readiness, and metrics endpoints

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;

use crate::metrics::encode_metrics;

/// Default staleness threshold in seconds (5 minutes)
const DEFAULT_STALE_THRESHOLD_SECS: u64 = 300;

/// Health check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub feed: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message_secs_ago: Option<u64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stale: bool,
}

/// Shared state for health endpoints
#[derive(Clone)]
pub struct ServerState {
    pub feed_name: String,
    pub connected: Arc<AtomicBool>,
    /// Unix timestamp (seconds) of last message received, 0 if none
    pub last_message_epoch_secs: Arc<AtomicU64>,
    /// Staleness threshold in seconds
    pub stale_threshold_secs: u64,
}

impl ServerState {
    pub fn new(
        feed_name: impl Into<String>,
        connected: Arc<AtomicBool>,
        last_message_epoch_secs: Arc<AtomicU64>,
    ) -> Self {
        Self {
            feed_name: feed_name.into(),
            connected,
            last_message_epoch_secs,
            stale_threshold_secs: DEFAULT_STALE_THRESHOLD_SECS,
        }
    }

    fn staleness_info(&self) -> (Option<u64>, bool) {
        let last_msg = self.last_message_epoch_secs.load(Ordering::SeqCst);
        if last_msg == 0 {
            return (None, false);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let secs_ago = now.saturating_sub(last_msg);
        let stale = secs_ago > self.stale_threshold_secs;
        (Some(secs_ago), stale)
    }
}

/// Health endpoint - returns 200 if running and not stale, 503 if stale
async fn health(State(state): State<ServerState>) -> (StatusCode, Json<HealthResponse>) {
    let connected = state.connected.load(Ordering::SeqCst);
    let (last_message_secs_ago, stale) = state.staleness_info();

    let unhealthy = stale && connected;
    let status_code = if unhealthy {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (
        status_code,
        Json(HealthResponse {
            status: if unhealthy { "stale" } else { "ok" }.to_string(),
            feed: state.feed_name.clone(),
            connected,
            last_message_secs_ago,
            stale,
        }),
    )
}

/// Ready endpoint - returns 200 only when connected and not stale
async fn ready(State(state): State<ServerState>) -> (StatusCode, Json<HealthResponse>) {
    let connected = state.connected.load(Ordering::SeqCst);
    let (last_message_secs_ago, stale) = state.staleness_info();

    let ready = connected && !stale;
    let status_code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let status = if !connected {
        "not_connected"
    } else if stale {
        "stale"
    } else {
        "ready"
    };

    (
        status_code,
        Json(HealthResponse {
            status: status.to_string(),
            feed: state.feed_name.clone(),
            connected,
            last_message_secs_ago,
            stale,
        }),
    )
}

/// Metrics endpoint - returns Prometheus text format
async fn metrics() -> impl IntoResponse {
    match encode_metrics() {
        Ok(body) => (
            StatusCode::OK,
            [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
            body,
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "text/plain; charset=utf-8")],
            format!("Failed to encode metrics: {}", e),
        ),
    }
}

/// Create the health server router
pub fn create_router(state: ServerState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/metrics", get(metrics))
        .with_state(state)
}

/// Run the health server
pub async fn run_server(addr: SocketAddr, state: ServerState) -> std::io::Result<()> {
    let app = create_router(state);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn create_test_state(connected: bool) -> ServerState {
        ServerState {
            feed_name: "test-feed".to_string(),
            connected: Arc::new(AtomicBool::new(connected)),
            last_message_epoch_secs: Arc::new(AtomicU64::new(0)),
            stale_threshold_secs: DEFAULT_STALE_THRESHOLD_SECS,
        }
    }

    fn create_test_state_with_last_message(
        connected: bool,
        last_msg_epoch: u64,
        threshold: u64,
    ) -> ServerState {
        ServerState {
            feed_name: "test-feed".to_string(),
            connected: Arc::new(AtomicBool::new(connected)),
            last_message_epoch_secs: Arc::new(AtomicU64::new(last_msg_epoch)),
            stale_threshold_secs: threshold,
        }
    }

    #[tokio::test]
    async fn test_health_returns_ok_when_fresh() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let state = create_test_state_with_last_message(true, now, 60);
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_returns_ok_when_no_messages_yet() {
        let state = create_test_state(true);
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_returns_503_when_stale() {
        let old_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 120;
        let state = create_test_state_with_last_message(true, old_time, 60);
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_ready_when_connected_and_fresh() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let state = create_test_state_with_last_message(true, now, 60);
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_when_disconnected() {
        let state = create_test_state(false);
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let state = create_test_state(true);
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response.headers().get("content-type").unwrap();
        assert!(content_type.to_str().unwrap().contains("text/plain"));
    }
}
