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

use crate::liveness;
use crate::metrics::encode_metrics;

/// Default staleness threshold in seconds (5 minutes)
/// If no messages received for this duration, health check reports stale
const DEFAULT_STALE_THRESHOLD_SECS: u64 = 300;

/// The three thresholds that drive the health endpoints, in seconds.
///
/// Liveness and readiness get their own threshold on purpose. They answer
/// different questions, so they need different answers: an operator raising the
/// liveness threshold to stop a sparse feed from tripping the kubelet probe must
/// not also push readiness out to the same value, because the data verdict is
/// part of what makes a silent feed visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthThresholds {
    /// Liveness (/health): no data AND no pong for this long means the socket is
    /// hung. Configured with `HEALTH_STALE_THRESHOLD_SECS`.
    pub stale_secs: u64,
    /// Readiness (/ready): no data for this long means the feed is serving
    /// nothing useful. Data only, never suppressed by a pong. Configured with
    /// `READY_STALE_THRESHOLD_SECS`.
    pub ready_secs: u64,
    /// Absolute ceiling on the data age. No pong can suppress this check, which
    /// makes it the only liveness signal the remote end does not control.
    /// Configured with `HEALTH_MAX_DATA_AGE_SECS`. See
    /// [`crate::liveness::evaluate_staleness`].
    pub max_data_age_secs: u64,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            stale_secs: DEFAULT_STALE_THRESHOLD_SECS,
            ready_secs: DEFAULT_STALE_THRESHOLD_SECS,
            max_data_age_secs: liveness::DEFAULT_MAX_DATA_AGE_SECS,
        }
    }
}

/// Health check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub feed: String,
    pub connected: bool,
    /// Seconds since the last WebSocket activity of any kind (None if none yet).
    /// Reported raw even when a pong keeps the connection healthy, so operators
    /// can still see how long a feed has been quiet.
    ///
    /// Control frames reset this, so it is NOT what the verdicts below use — see
    /// `data_age_secs`. The two match whenever data is flowing. When they do not,
    /// the gap is itself the diagnosis: frames arriving, no data behind them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message_secs_ago: Option<u64>,
    /// Seconds since a message was last forwarded downstream (None if none yet, or
    /// the clock stepped backwards and no trustworthy age can be computed).
    ///
    /// This is the age every verdict below is computed from, including the
    /// absolute ceiling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_age_secs: Option<u64>,
    /// Seconds since the last WebSocket pong (None if none received, or the
    /// connector does not track pongs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pong_age_secs: Option<u64>,
    /// Liveness verdict: neither data nor a pong within the threshold, or the data
    /// age reached the absolute ceiling. This is what /health turns into a 503.
    ///
    /// Always serialized, and identical on both endpoints. It used to be skipped
    /// when false and to mean the data verdict on /ready, so one field name
    /// carried two meanings depending on the URL.
    pub stale: bool,
    /// Data verdict: no market data within the threshold, whatever the socket is
    /// doing. This is what /ready turns into a 503. On /health it is the
    /// "quiet but alive" flag when `stale` is false.
    pub data_stale: bool,
}

/// Shared state for health endpoints
#[derive(Clone)]
pub struct ServerState {
    pub feed_name: String,
    pub connected: Arc<AtomicBool>,
    /// Unix timestamp (seconds) of the last WebSocket activity, 0 if none.
    ///
    /// Any received frame updates this, including the control frames that are
    /// counted and dropped. Reported to operators, never used for a verdict.
    pub last_message_epoch_secs: Arc<AtomicU64>,
    /// Unix timestamp (seconds) at which a message was last forwarded downstream,
    /// 0 before the connection is up.
    ///
    /// Every verdict here reads this clock rather than the activity one above. The
    /// two coincide whenever data is flowing, so the endpoints behave identically
    /// in the healthy case; they diverge exactly when a peer answers our control
    /// frames while publishing nothing, which is what the ceiling exists to catch
    /// and what an activity-driven verdict cannot see. See [`crate::liveness`].
    pub last_data_epoch_secs: Arc<AtomicU64>,
    /// Unix timestamp (seconds) of the last WebSocket pong, 0 if none received yet
    /// or the connector does not track pongs
    pub last_pong_epoch_secs: Arc<AtomicU64>,
    /// Thresholds behind the two endpoints.
    pub thresholds: HealthThresholds,
}

/// Staleness verdicts shared by the health endpoints.
struct Staleness {
    /// Seconds since the last WebSocket activity of any kind, None if none yet.
    last_message_secs_ago: Option<u64>,
    /// Seconds since a message was last forwarded downstream, None when no
    /// trustworthy age can be computed. Every verdict below is derived from this.
    data_age_secs: Option<u64>,
    /// Seconds since the last pong, None if none received yet.
    pong_age_secs: Option<u64>,
    /// No market data within the readiness threshold (or past the ceiling),
    /// regardless of socket liveness. Used by /ready.
    data_stale: bool,
    /// Neither data nor a pong within the liveness threshold, or the data age
    /// reached the ceiling, so the connection is dead. Used by /health.
    stale: bool,
}

impl ServerState {
    pub fn new(feed_name: impl Into<String>, connected: Arc<AtomicBool>) -> Self {
        Self {
            feed_name: feed_name.into(),
            connected,
            last_message_epoch_secs: Arc::new(AtomicU64::new(0)),
            last_data_epoch_secs: Arc::new(AtomicU64::new(0)),
            last_pong_epoch_secs: Arc::new(AtomicU64::new(0)),
            thresholds: HealthThresholds::default(),
        }
    }

    /// Create with custom thresholds and shared activity/data/pong handles.
    ///
    /// `last_data_epoch_secs` should be `Runner::data_handle` and `last_pong_epoch_secs`
    /// the connector pong handle, so /health can tell a quiet market on a healthy
    /// socket from a hung socket, and tell both from a peer that answers control
    /// frames while publishing nothing. Pass a zero-valued pong handle for
    /// connectors that do not track pongs: /health then falls back to data age.
    ///
    /// ORDERING: all three handles are read once, here. Every one of them must
    /// already be the counter the connector will actually write to — a handle that
    /// a connector allocates later, inside `connect()`, would be captured as a
    /// permanently zero `Arc` and would silently switch its check off with nothing
    /// to fail on. `Runner::data_handle` and `Runner::pong_handle` re-read the
    /// connector on every call for exactly this reason, so call them as late as
    /// possible and never cache their results across `connect()`.
    pub fn with_last_message(
        feed_name: impl Into<String>,
        connected: Arc<AtomicBool>,
        last_message_epoch_secs: Arc<AtomicU64>,
        last_data_epoch_secs: Arc<AtomicU64>,
        last_pong_epoch_secs: Arc<AtomicU64>,
        thresholds: HealthThresholds,
    ) -> Self {
        Self {
            feed_name: feed_name.into(),
            connected,
            last_message_epoch_secs,
            last_data_epoch_secs,
            last_pong_epoch_secs,
            thresholds,
        }
    }

    /// Calculate staleness info from current time
    fn staleness_info(&self) -> Staleness {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let pong_age_secs =
            liveness::pong_age_secs(self.last_pong_epoch_secs.load(Ordering::SeqCst), now);

        let last_msg = self.last_message_epoch_secs.load(Ordering::SeqCst);
        if last_msg == 0 {
            // No messages yet - not stale (just started)
            return Staleness {
                last_message_secs_ago: None,
                data_age_secs: None,
                pong_age_secs,
                data_stale: false,
                stale: false,
            };
        }

        let secs_ago = now.saturating_sub(last_msg);

        // Age of the last message actually forwarded downstream. NOT secs_ago,
        // which any received frame resets: this connector re-subscribes on every
        // 15-minute rollover, so a peer that acks those subscriptions and publishes
        // nothing would keep the activity clock fresh right through a data drought
        // and hold the ceiling open forever.
        let data_age_secs = match liveness::data_age_signal(
            self.last_data_epoch_secs.load(Ordering::SeqCst),
            now,
        ) {
            liveness::DataAgeSignal::Age(age_secs) => age_secs,
            // Nothing forwarded yet, or a clock that stepped backwards. Neither
            // yields an age we can compare, and neither is evidence the socket is
            // dead, so both endpoints stay green — the same answer this endpoint
            // gave before the ceiling existed. The watchdog in this process reads
            // the same clock every 15s and logs the skew (reason =
            // "data_clock_skew"); this handler has no rate limiter and would emit one
            // line per kubelet probe, so it does not duplicate that warn.
            liveness::DataAgeSignal::Uninitialized
            | liveness::DataAgeSignal::FutureEpoch { .. } => {
                return Staleness {
                    last_message_secs_ago: Some(secs_ago),
                    data_age_secs: None,
                    pong_age_secs,
                    data_stale: false,
                    stale: false,
                }
            }
        };

        // The two verdicts run the same rule twice, differing only in the threshold
        // and in whether the pong is allowed to speak. Exclusive bound preserves the
        // original `>` comparison of this endpoint.

        // Liveness: alive if EITHER data or a pong is recent, the same rule the
        // watchdog applies, plus the data-age ceiling that a pong cannot suppress.
        let live = liveness::evaluate_staleness(
            data_age_secs,
            pong_age_secs,
            self.thresholds.stale_secs,
            self.thresholds.max_data_age_secs,
            liveness::ThresholdBound::Exclusive,
        );
        // Readiness: data only. Passing `None` for the pong is what makes that
        // explicit — no socket-level signal can make a silent feed look ready — and
        // it keeps the ceiling applying here too, since the ceiling is itself a
        // data-age rule.
        let ready = liveness::evaluate_staleness(
            data_age_secs,
            None,
            self.thresholds.ready_secs,
            self.thresholds.max_data_age_secs,
            liveness::ThresholdBound::Exclusive,
        );
        Staleness {
            last_message_secs_ago: Some(secs_ago),
            data_age_secs: Some(data_age_secs),
            pong_age_secs,
            data_stale: ready.stale,
            stale: live.stale,
        }
    }
}

/// Health endpoint (kubelet liveness probe) - returns 200 while the connection is alive.
///
/// Returns 503 when NEITHER market data NOR a WebSocket pong arrived within the
/// threshold, which means the socket is genuinely hung, and also when the data age
/// alone reaches the absolute ceiling. A quiet market on a healthy socket is NOT
/// unhealthy: sparse feeds such as the Kalshi 15-minute crypto contracts idle for
/// minutes between windows, and restarting them loses archive data. But the pong
/// is emitted by the exchange, so it may only postpone the restart up to the
/// ceiling, never cancel it: otherwise a peer that acks and then falls silent
/// keeps this endpoint green forever while the connector captures nothing.
async fn health(State(state): State<ServerState>) -> (StatusCode, Json<HealthResponse>) {
    let connected = state.connected.load(Ordering::SeqCst);
    let staleness = state.staleness_info();

    // Unhealthy if stale AND was previously connected (not just starting up)
    let unhealthy = staleness.stale && connected;
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
            last_message_secs_ago: staleness.last_message_secs_ago,
            data_age_secs: staleness.data_age_secs,
            pong_age_secs: staleness.pong_age_secs,
            stale: staleness.stale,
            data_stale: staleness.data_stale,
        }),
    )
}

/// Ready endpoint - returns 200 only when connected and actually receiving data.
///
/// Deliberately STRICTER than /health: readiness uses the data-only verdict, with no
/// pong fallback. A connector whose socket is alive but which has no data is not
/// serving anything useful, so it should drop out of the ready set — but it must NOT
/// be restarted, which is why /health (the liveness probe) accepts a fresh pong.
///
/// Do not collapse the two verdicts into one. Giving /health the data-only rule
/// restarts healthy quiet feeds (the failure this split exists to prevent); giving
/// /ready the pong rule advertises a silent feed as ready.
async fn ready(State(state): State<ServerState>) -> (StatusCode, Json<HealthResponse>) {
    let connected = state.connected.load(Ordering::SeqCst);
    let staleness = state.staleness_info();

    // Ready only if connected and receiving data
    let ready = connected && !staleness.data_stale;
    let status_code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let status = if !connected {
        "not_connected"
    } else if staleness.data_stale {
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
            last_message_secs_ago: staleness.last_message_secs_ago,
            data_age_secs: staleness.data_age_secs,
            pong_age_secs: staleness.pong_age_secs,
            // Both verdicts, with the same meaning as on /health. The HTTP status
            // above is the one that differs: readiness is decided by data_stale.
            stale: staleness.stale,
            data_stale: staleness.data_stale,
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
        create_test_state_with_thresholds(connected, 0, 0, HealthThresholds::default())
    }

    /// State with no pong tracked, which exercises the data-only fallback.
    fn create_test_state_with_last_message(
        connected: bool,
        last_msg_epoch: u64,
        threshold: u64,
    ) -> ServerState {
        create_test_state_with_pong(connected, last_msg_epoch, 0, threshold)
    }

    fn create_test_state_with_pong(
        connected: bool,
        last_msg_epoch: u64,
        last_pong_epoch: u64,
        threshold: u64,
    ) -> ServerState {
        create_test_state_with_ceiling(
            connected,
            last_msg_epoch,
            last_pong_epoch,
            threshold,
            CEILING,
        )
    }

    fn create_test_state_with_ceiling(
        connected: bool,
        last_msg_epoch: u64,
        last_pong_epoch: u64,
        threshold: u64,
        max_data_age_secs: u64,
    ) -> ServerState {
        create_test_state_with_thresholds(
            connected,
            last_msg_epoch,
            last_pong_epoch,
            HealthThresholds {
                stale_secs: threshold,
                // Most tests predate the split and expect one threshold to drive
                // both endpoints, which is what the operator gets by default.
                ready_secs: threshold,
                max_data_age_secs,
            },
        )
    }

    /// The healthy shape: every received frame was data, so the activity clock and
    /// the data clock agree. Tests that predate the split use this.
    fn create_test_state_with_thresholds(
        connected: bool,
        last_msg_epoch: u64,
        last_pong_epoch: u64,
        thresholds: HealthThresholds,
    ) -> ServerState {
        create_test_state_with_split_clocks(
            connected,
            last_msg_epoch,
            last_msg_epoch,
            last_pong_epoch,
            thresholds,
        )
    }

    /// Activity and data clocks set independently, which is the only way to
    /// express a peer that keeps sending control frames while publishing nothing.
    fn create_test_state_with_split_clocks(
        connected: bool,
        last_msg_epoch: u64,
        last_data_epoch: u64,
        last_pong_epoch: u64,
        thresholds: HealthThresholds,
    ) -> ServerState {
        ServerState {
            feed_name: "test-feed".to_string(),
            connected: Arc::new(AtomicBool::new(connected)),
            last_message_epoch_secs: Arc::new(AtomicU64::new(last_msg_epoch)),
            last_data_epoch_secs: Arc::new(AtomicU64::new(last_data_epoch)),
            last_pong_epoch_secs: Arc::new(AtomicU64::new(last_pong_epoch)),
            thresholds,
        }
    }

    /// Absolute data-age ceiling (HEALTH_MAX_DATA_AGE_SECS default).
    const CEILING: u64 = liveness::DEFAULT_MAX_DATA_AGE_SECS;

    fn now_epoch() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX_EPOCH")
            .as_secs()
    }

    async fn get(state: ServerState, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = create_router(state)
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn test_health_returns_ok_when_fresh() {
        // Connected with recent message
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let state = create_test_state_with_last_message(true, now, 60);
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_returns_ok_when_no_messages_yet() {
        // Connected but no messages yet (just started)
        let state = create_test_state(true);
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_returns_503_when_stale() {
        // Connected but stale (old message)
        let old_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() - 120; // 2 minutes ago
        let state = create_test_state_with_last_message(true, old_time, 60); // 60s threshold
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
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
    async fn test_ready_when_stale() {
        let old_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() - 120;
        let state = create_test_state_with_last_message(true, old_time, 60);
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
        // Verify Prometheus content type
        let content_type = response.headers().get("content-type").unwrap();
        assert!(content_type.to_str().unwrap().contains("text/plain"));
    }

    #[test]
    fn test_staleness_info_no_messages() {
        let state = create_test_state(true);
        let staleness = state.staleness_info();
        assert!(staleness.last_message_secs_ago.is_none());
        assert!(staleness.pong_age_secs.is_none());
        assert!(!staleness.stale);
        assert!(!staleness.data_stale);
    }

    #[test]
    fn test_staleness_info_fresh() {
        let state = create_test_state_with_last_message(true, now_epoch(), 60);
        let staleness = state.staleness_info();
        assert!(staleness.last_message_secs_ago.is_some());
        assert!(staleness.last_message_secs_ago.unwrap() <= 1);
        assert!(!staleness.stale);
        assert!(!staleness.data_stale);
    }

    #[test]
    fn test_staleness_info_stale() {
        let state = create_test_state_with_last_message(true, now_epoch() - 120, 60);
        let staleness = state.staleness_info();
        assert!(staleness.last_message_secs_ago.is_some());
        assert!(staleness.last_message_secs_ago.unwrap() >= 119);
        assert!(staleness.stale);
        assert!(staleness.data_stale);
    }

    #[test]
    fn test_staleness_info_idle_data_with_fresh_pong_is_not_stale() {
        // Quiet market, healthy socket: liveness is fine, readiness is not.
        let now = now_epoch();
        let state = create_test_state_with_pong(true, now - 400, now, 300);
        let staleness = state.staleness_info();
        assert!(!staleness.stale, "a fresh pong proves the socket is alive");
        assert!(staleness.data_stale, "no data is still not ready");
        assert!(staleness.last_message_secs_ago.unwrap() >= 399);
        assert!(staleness.pong_age_secs.unwrap() <= 1);
    }

    #[tokio::test]
    async fn test_health_ok_when_data_idle_but_pong_fresh() {
        // The outage case: sparse 15-minute markets, no data for 400s, socket answering
        // pings. A 503 here makes the kubelet restart a healthy connector.
        let now = now_epoch();
        let state = create_test_state_with_pong(true, now - 400, now, 300);

        let (status, body) = get(state, "/health").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn test_health_503_when_data_idle_and_pong_stale() {
        // Genuinely hung socket: neither data nor pong. Must still be restarted.
        let now = now_epoch();
        let state = create_test_state_with_pong(true, now - 400, now - 400, 300);

        let (status, body) = get(state, "/health").await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "stale");
    }

    #[tokio::test]
    async fn test_health_ok_when_data_flowing_despite_stale_pong() {
        // Data alone proves liveness; a missing pong does not override it.
        let now = now_epoch();
        let state = create_test_state_with_pong(true, now, now - 400, 300);

        let (status, _body) = get(state, "/health").await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_503_when_no_pong_yet_and_data_stale() {
        // Pong epoch 0 means "never received", not "just now": it must not mask
        // stale data. This is the pre-pong behaviour, preserved.
        let now = now_epoch();
        let state = create_test_state_with_pong(true, now - 400, 0, 300);

        let (status, body) = get(state, "/health").await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["pong_age_secs"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_health_body_reports_raw_data_age_and_pong_age() {
        // Operators still want the data age even when the socket is healthy.
        let now = now_epoch();
        let state = create_test_state_with_pong(true, now - 400, now, 300);

        let (_status, body) = get(state, "/health").await;

        assert!(body["last_message_secs_ago"].as_u64().unwrap() >= 399);
        assert!(body["pong_age_secs"].as_u64().unwrap() <= 1);
        // Both verdicts are always present, and they disagree here on purpose:
        // the socket is alive (stale=false) while the feed is silent
        // (data_stale=true).
        assert_eq!(body["stale"], false);
        assert_eq!(body["data_stale"], true);
    }

    #[tokio::test]
    async fn test_ready_not_ready_when_data_stale_even_with_fresh_pong() {
        // /ready stays data-only on purpose: a silent feed serves nothing useful,
        // even though /health must not restart it.
        let now = now_epoch();
        let state = create_test_state_with_pong(true, now - 400, now, 300);

        let (status, body) = get(state, "/ready").await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "stale");
        // One name, one meaning, on both endpoints: `stale` is always the liveness
        // verdict and `data_stale` is always the data verdict. The readiness
        // decision is data_stale, which is why this is a 503 while stale is false.
        assert_eq!(body["data_stale"], true);
        assert_eq!(body["stale"], false);
    }

    #[tokio::test]
    async fn test_health_503_when_data_reaches_the_ceiling_despite_a_fresh_pong() {
        // REGRESSION GUARD: a peer that answers control frames while publishing
        // nothing kept /health at 200 forever, so the pod ran indefinitely
        // capturing nothing. The data-age ceiling is what ends it.
        let now = now_epoch();
        let state = create_test_state_with_ceiling(true, now - (CEILING + 60), now, 300, CEILING);

        let (status, body) = get(state, "/health").await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "stale");
        assert_eq!(body["stale"], true);
        assert_eq!(body["data_stale"], true);
        assert!(
            body["pong_age_secs"].as_u64().unwrap() <= 1,
            "the pong is fresh and still cannot save it"
        );
    }

    #[tokio::test]
    async fn test_health_ok_just_below_the_ceiling() {
        // A sparse feed may idle well past the normal threshold; only the ceiling
        // ends it, so the crashloop this change fixes does not come back.
        let now = now_epoch();
        let state = create_test_state_with_ceiling(true, now - (CEILING - 60), now, 300, CEILING);

        let (status, body) = get(state, "/health").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["stale"], false);
        assert_eq!(body["data_stale"], true);
    }

    #[tokio::test]
    async fn test_liveness_threshold_does_not_move_readiness() {
        // MEDIUM regression guard. HEALTH_STALE_THRESHOLD_SECS is there so an
        // operator can stop a sparse feed from tripping the liveness probe. When one
        // field drove both endpoints, doing that also pushed /ready out to the same
        // value and silently switched off the data-stale signal, which is one of the
        // two things backing up the ceiling. The two thresholds are now independent.
        let now = now_epoch();
        let state = create_test_state_with_thresholds(
            true,
            now - 400,
            now,
            HealthThresholds {
                stale_secs: 1800,
                ready_secs: 300,
                max_data_age_secs: CEILING,
            },
        );

        let (health_status, health_body) = get(state.clone(), "/health").await;
        assert_eq!(
            health_status,
            StatusCode::OK,
            "the raised liveness threshold does its job"
        );
        assert_eq!(health_body["stale"], false);

        let (ready_status, ready_body) = get(state, "/ready").await;
        assert_eq!(
            ready_status,
            StatusCode::SERVICE_UNAVAILABLE,
            "but readiness keeps its own, tighter threshold"
        );
        assert_eq!(ready_body["data_stale"], true);
    }

    #[test]
    fn test_readiness_honours_the_ceiling_under_the_default_configuration() {
        // The ceiling is itself a data-age rule, so it applies to the data verdict
        // too: past the ceiling a feed is neither alive nor ready.
        let now = now_epoch();
        let state = create_test_state_with_thresholds(
            true,
            now - (CEILING + 60),
            now,
            HealthThresholds {
                stale_secs: 300,
                ready_secs: 300,
                max_data_age_secs: CEILING,
            },
        );

        let staleness = state.staleness_info();

        assert!(staleness.data_stale, "the ceiling marks the feed not ready");
        assert!(staleness.stale, "and the liveness probe restarts it");
    }

    #[test]
    fn test_readiness_threshold_above_the_ceiling_governs_readiness() {
        // Precedence worth pinning: the never-an-accelerant guard applies on this
        // path too, so a readiness threshold above the ceiling raises the ceiling
        // rather than the other way round. An operator who deliberately asks for a
        // tolerant readiness signal gets one. Nothing is at risk: readiness never
        // restarts anything, and liveness still ends the pod at its own ceiling.
        let now = now_epoch();
        let state = create_test_state_with_thresholds(
            true,
            now - (CEILING + 60),
            now,
            HealthThresholds {
                stale_secs: 300,
                ready_secs: 3600,
                max_data_age_secs: CEILING,
            },
        );

        let staleness = state.staleness_info();

        assert!(!staleness.data_stale, "readiness follows its own threshold");
        assert!(
            staleness.stale,
            "liveness still restarts the pod at the ceiling"
        );
    }

    #[tokio::test]
    async fn test_health_503_when_only_control_frames_arrive_past_the_ceiling() {
        // THE regression guard for the endpoint. The activity clock is fresh
        // because the peer keeps answering our 15-minute resubscribes, and the pong
        // is fresh because it keeps answering pings. Only the data clock is old.
        // Reading the activity clock here — which is what the endpoint used to do —
        // gives 200 forever while the connector archives nothing.
        let now = now_epoch();
        let state = create_test_state_with_split_clocks(
            true,
            now - 5,
            now - (CEILING + 60),
            now,
            HealthThresholds {
                stale_secs: 300,
                ready_secs: 300,
                max_data_age_secs: CEILING,
            },
        );

        let (status, body) = get(state, "/health").await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["stale"], true);
        assert_eq!(body["data_stale"], true);
        assert!(
            body["last_message_secs_ago"].as_u64().unwrap() <= 6,
            "frames really are still arriving, which is the point"
        );
        assert!(body["data_age_secs"].as_u64().unwrap() >= CEILING);
    }

    #[tokio::test]
    async fn test_ready_503_when_only_control_frames_arrive() {
        // Same split well below the ceiling: readiness is a data verdict, so it must
        // drop out long before the ceiling does anything.
        let now = now_epoch();
        let state = create_test_state_with_split_clocks(
            true,
            now - 5,
            now - 400,
            now,
            HealthThresholds {
                stale_secs: 300,
                ready_secs: 300,
                max_data_age_secs: CEILING,
            },
        );

        let (status, body) = get(state, "/ready").await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["data_stale"], true);
        assert_eq!(
            body["stale"], false,
            "the pong still says the socket is alive, so do not restart it yet"
        );
    }

    #[test]
    fn test_control_frames_alone_cannot_hold_the_ceiling_open() {
        // Simulates the 15-minute rollover cadence directly: an ack lands every 900s
        // and moves the activity clock, while the data clock does not move at all.
        // Under the old rule each ack reset the age and the 1800s ceiling was never
        // reachable.
        let now = now_epoch();
        let data_epoch = now - (CEILING + 1);
        for acks_ago in [0_u64, 900, 1_800] {
            let state = create_test_state_with_split_clocks(
                true,
                now - acks_ago,
                data_epoch,
                now,
                HealthThresholds {
                    stale_secs: 300,
                    ready_secs: 300,
                    max_data_age_secs: CEILING,
                },
            );
            let staleness = state.staleness_info();
            assert!(
                staleness.stale,
                "ack {acks_ago}s ago must not vouch for data {} s old",
                CEILING + 1
            );
        }
    }

    #[test]
    fn test_verdicts_are_suppressed_when_the_data_clock_is_in_the_future() {
        // A backwards clock step. Every age is untrustworthy, so no verdict is
        // possible: reporting age 0 would look identical to a perfectly healthy
        // feed, with nothing anywhere saying otherwise. The watchdog in the same
        // process logs reason = "data_clock_skew".
        let now = now_epoch();
        let state = create_test_state_with_split_clocks(
            true,
            now - 400,
            now + 600,
            now - 400,
            HealthThresholds {
                stale_secs: 300,
                ready_secs: 300,
                max_data_age_secs: CEILING,
            },
        );

        let staleness = state.staleness_info();

        assert!(!staleness.stale);
        assert!(!staleness.data_stale);
        assert_eq!(
            staleness.data_age_secs, None,
            "no age is reported rather than a fabricated zero"
        );
    }

    #[test]
    fn test_ceiling_below_the_threshold_never_fires_early() {
        // Misconfiguration guard: a ceiling under the normal threshold must not
        // start killing quiet feeds earlier than the unfixed code did.
        let now = now_epoch();
        let state = create_test_state_with_ceiling(true, now - 100, now, 300, 60);

        let staleness = state.staleness_info();

        assert!(!staleness.stale);
        assert!(!staleness.data_stale);
    }
}
