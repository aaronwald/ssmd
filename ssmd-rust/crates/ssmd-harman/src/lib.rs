pub mod api;
pub mod pump;
pub mod shutdown;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use deadpool_postgres::Pool;
use tokio::sync::{RwLock, Semaphore};

use ssmd_harman_ems::Ems;
use ssmd_harman_oms::Oms;
use ssmd_harman_oms::runner::{OmsRunner, PumpTrigger};

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
    pub auth_cache: RwLock<HashMap<u64, CachedAuth>>,
    pub key_sessions: DashMap<String, i64>,
    /// Prevents concurrent pump execution for static-token auth (fallback).
    pub pump_semaphore: Semaphore,
}
