pub mod api;
pub mod pump;
pub mod reconciliation;
pub mod recovery;
pub mod shutdown;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use deadpool_postgres::Pool;
use tokio::sync::{RwLock, Semaphore};

use harman::exchange::ExchangeAdapter;
use harman::risk::RiskLimits;

/// Cached auth validation result from data-ts
pub struct CachedAuth {
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub cached_at: Instant,
}

/// Per-request session context, injected by auth middleware
#[derive(Clone, Debug)]
pub struct SessionContext {
    pub session_id: i64,
    pub scopes: Vec<String>,
    pub key_prefix: String,
}

/// Metrics for prometheus
pub struct Metrics {
    pub registry: prometheus::Registry,
    pub orders_dequeued: prometheus::IntCounter,
    pub orders_submitted: prometheus::IntCounter,
    pub orders_rejected: prometheus::IntCounter,
    pub orders_cancelled: prometheus::IntCounter,
    pub fills_recorded: prometheus::IntCounter,
    pub reconciliation_ok: prometheus::IntCounter,
    pub reconciliation_mismatch: prometheus::IntCounterVec,
    pub reconciliation_duration: prometheus::Histogram,
    pub reconciliation_last_success: prometheus::IntGauge,
    pub reconciliation_fills_discovered: prometheus::IntCounter,
    pub orders_amended: prometheus::IntCounter,
    pub orders_decreased: prometheus::IntCounter,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = prometheus::Registry::new();

        let orders_dequeued =
            prometheus::IntCounter::new("harman_orders_dequeued_total", "Orders dequeued from queue")
                .unwrap();
        let orders_submitted =
            prometheus::IntCounter::new("harman_orders_submitted_total", "Orders submitted to exchange")
                .unwrap();
        let orders_rejected =
            prometheus::IntCounter::new("harman_orders_rejected_total", "Orders rejected by exchange")
                .unwrap();
        let orders_cancelled =
            prometheus::IntCounter::new("harman_orders_cancelled_total", "Orders cancelled")
                .unwrap();
        let fills_recorded =
            prometheus::IntCounter::new("harman_fills_recorded_total", "Fills recorded")
                .unwrap();
        let reconciliation_ok =
            prometheus::IntCounter::new("harman_reconciliation_ok_total", "Successful reconciliation cycles")
                .unwrap();
        let reconciliation_mismatch = prometheus::IntCounterVec::new(
            prometheus::Opts::new("harman_reconciliation_mismatch_total", "Position mismatches detected"),
            &["severity"],
        )
        .unwrap();
        let reconciliation_duration = prometheus::Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "harman_reconciliation_duration_seconds",
                "Reconciliation cycle duration",
            )
            .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]),
        )
        .unwrap();
        let reconciliation_last_success = prometheus::IntGauge::new(
            "harman_reconciliation_last_success_timestamp",
            "Epoch seconds of last successful reconciliation",
        )
        .unwrap();
        let reconciliation_fills_discovered = prometheus::IntCounter::new(
            "harman_reconciliation_fills_discovered_total",
            "Fills discovered during reconciliation",
        )
        .unwrap();
        let orders_amended =
            prometheus::IntCounter::new("harman_orders_amended_total", "Orders amended on exchange")
                .unwrap();
        let orders_decreased = prometheus::IntCounter::new(
            "harman_orders_decreased_total",
            "Orders decreased on exchange",
        )
        .unwrap();

        registry.register(Box::new(orders_dequeued.clone())).unwrap();
        registry.register(Box::new(orders_submitted.clone())).unwrap();
        registry.register(Box::new(orders_rejected.clone())).unwrap();
        registry.register(Box::new(orders_cancelled.clone())).unwrap();
        registry.register(Box::new(fills_recorded.clone())).unwrap();
        registry.register(Box::new(reconciliation_ok.clone())).unwrap();
        registry.register(Box::new(reconciliation_mismatch.clone())).unwrap();
        registry.register(Box::new(reconciliation_duration.clone())).unwrap();
        registry.register(Box::new(reconciliation_last_success.clone())).unwrap();
        registry.register(Box::new(reconciliation_fills_discovered.clone())).unwrap();
        registry.register(Box::new(orders_amended.clone())).unwrap();
        registry.register(Box::new(orders_decreased.clone())).unwrap();

        Self {
            registry,
            orders_dequeued,
            orders_submitted,
            orders_rejected,
            orders_cancelled,
            fills_recorded,
            reconciliation_ok,
            reconciliation_mismatch,
            reconciliation_duration,
            reconciliation_last_success,
            reconciliation_fills_discovered,
            orders_amended,
            orders_decreased,
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared application state
pub struct AppState {
    pub pool: Pool,
    pub exchange: Arc<dyn ExchangeAdapter>,
    pub risk_limits: RiskLimits,
    pub shutting_down: AtomicBool,
    pub metrics: Metrics,
    // Static tokens (backward compat, used when AUTH_VALIDATE_URL is not set)
    pub api_token: String,
    pub admin_token: String,
    // Startup session (used for static token auth + recovery)
    pub startup_session_id: i64,
    // HTTP auth validation (new)
    pub auth_validate_url: Option<String>,
    pub http_client: reqwest::Client,
    // Per-session state
    pub session_semaphores: DashMap<i64, Arc<Semaphore>>,
    pub suspended_sessions: DashMap<i64, ()>,
    // Caches
    pub auth_cache: RwLock<HashMap<u64, CachedAuth>>,
    pub key_sessions: DashMap<String, i64>,
    /// Prevents concurrent pump execution for static-token auth (fallback).
    pub pump_semaphore: Semaphore,
}
