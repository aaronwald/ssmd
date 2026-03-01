pub mod api;
pub mod pump;
pub mod shutdown;

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use deadpool_postgres::Pool;
use lru::LruCache;
use tokio::sync::{RwLock, Semaphore};

use ssmd_harman_ems::Ems;
use ssmd_harman_oms::Oms;
use ssmd_harman_oms::runner::{OmsRunner, PumpTrigger};

/// Prometheus metrics for monitor endpoints
pub struct MonitorMetrics {
    pub requests_total: prometheus::IntCounterVec,
    pub redis_duration_seconds: prometheus::Histogram,
    pub redis_errors_total: prometheus::IntCounter,
}

impl MonitorMetrics {
    pub fn new(registry: &prometheus::Registry) -> Self {
        let requests_total = prometheus::IntCounterVec::new(
            prometheus::Opts::new(
                "harman_monitor_requests_total",
                "Total monitor endpoint requests",
            ),
            &["endpoint", "status"],
        )
        .unwrap();
        let redis_duration_seconds = prometheus::Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "harman_monitor_redis_duration_seconds",
                "Redis operation duration for monitor endpoints",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25]),
        )
        .unwrap();
        let redis_errors_total = prometheus::IntCounter::new(
            "harman_monitor_redis_errors_total",
            "Total Redis errors in monitor endpoints",
        )
        .unwrap();

        registry
            .register(Box::new(requests_total.clone()))
            .unwrap();
        registry
            .register(Box::new(redis_duration_seconds.clone()))
            .unwrap();
        registry
            .register(Box::new(redis_errors_total.clone()))
            .unwrap();

        Self {
            requests_total,
            redis_duration_seconds,
            redis_errors_total,
        }
    }
}

/// Cloudflare Access JWKS key (RSA)
pub struct CfJwk {
    pub kid: String,
    pub n: String,
    pub e: String,
}

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

/// Shared application state
pub struct AppState {
    /// The EMS instance (owns exchange, risk_limits, shutting_down, pump)
    pub ems: Arc<Ems>,
    /// The OMS instance (owns reconciliation, recovery, positions, suspended sessions)
    pub oms: Arc<Oms>,
    pub pool: Pool,
    /// Shared prometheus registry for all metrics
    pub registry: prometheus::Registry,
    // Static tokens (backward compat, used when AUTH_VALIDATE_URL is not set)
    pub api_token: String,
    pub admin_token: String,
    // Startup session (used for static token auth + recovery)
    pub startup_session_id: i64,
    // HTTP auth validation (new)
    pub auth_validate_url: Option<String>,
    pub http_client: reqwest::Client,
    // OMS background runner
    pub runner: Arc<OmsRunner>,
    pub auto_pump: bool,
    pub pump_trigger: PumpTrigger,
    // Per-session state
    pub session_semaphores: DashMap<i64, Arc<Semaphore>>,
    // Caches
    pub auth_cache: RwLock<LruCache<String, CachedAuth>>,
    pub key_sessions: DashMap<String, i64>,
    /// Cached ticker list from secmaster (via data-ts), refreshed every 5 minutes
    pub ticker_cache: RwLock<Option<(std::time::Instant, Vec<String>)>>,
    /// Prevents concurrent pump execution for static-token auth (fallback).
    pub pump_semaphore: Semaphore,
    /// Optional Redis connection for monitor data (from ssmd-cache)
    pub redis_conn: Option<redis::aio::MultiplexedConnection>,
    /// Prometheus metrics for monitor endpoints
    pub monitor_metrics: MonitorMetrics,
    /// Exchange type (e.g., "kalshi")
    pub exchange_type: String,
    /// Exchange environment (e.g., "demo" or "prod")
    pub environment: String,
    // Cloudflare Access JWT auth (Path 4)
    pub cf_jwks_url: Option<String>,
    pub cf_aud: Option<String>,
    pub cf_jwks: RwLock<Option<(Instant, Vec<CfJwk>)>>,
    pub data_ts_api_key: Option<String>,
    pub data_ts_base_url: Option<String>,
}

impl AppState {
    pub fn new_auth_cache() -> LruCache<String, CachedAuth> {
        LruCache::new(NonZeroUsize::new(512).unwrap())
    }
}
