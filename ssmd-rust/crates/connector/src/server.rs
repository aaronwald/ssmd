use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;

/// Health check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub feed: String,
    pub connected: bool,
}

/// Metrics response (placeholder for future Prometheus integration)
#[derive(Serialize)]
pub struct MetricsResponse {
    pub messages_received: u64,
    pub messages_written: u64,
    pub errors: u64,
}

/// Shared state for health endpoints
#[derive(Clone)]
pub struct ServerState {
    pub feed_name: String,
    pub connected: Arc<AtomicBool>,
}

impl ServerState {
    pub fn new(feed_name: impl Into<String>, connected: Arc<AtomicBool>) -> Self {
        Self {
            feed_name: feed_name.into(),
            connected,
        }
    }
}

/// Health endpoint - always returns 200 if server is running
async fn health(State(state): State<ServerState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        feed: state.feed_name.clone(),
        connected: state.connected.load(Ordering::SeqCst),
    })
}

/// Ready endpoint - returns 200 only when connected
async fn ready(State(state): State<ServerState>) -> (StatusCode, Json<HealthResponse>) {
    let connected = state.connected.load(Ordering::SeqCst);
    let status_code = if connected {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(HealthResponse {
            status: if connected { "ready" } else { "not_ready" }.to_string(),
            feed: state.feed_name.clone(),
            connected,
        }),
    )
}

/// Metrics endpoint - placeholder for future Prometheus integration
async fn metrics() -> Json<MetricsResponse> {
    // TODO: Integrate with actual metrics collection
    Json(MetricsResponse {
        messages_received: 0,
        messages_written: 0,
        errors: 0,
    })
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
        }
    }

    #[tokio::test]
    async fn test_health_returns_ok() {
        let state = create_test_state(true);
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_when_connected() {
        let state = create_test_state(true);
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_when_disconnected() {
        let state = create_test_state(false);
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let state = create_test_state(true);
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
