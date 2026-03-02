pub mod groups;
pub mod positions;
pub mod reconciliation;
pub mod recovery;
pub mod runner;

use std::sync::Arc;

use dashmap::DashMap;
use deadpool_postgres::Pool;

use harman::exchange::ExchangeAdapter;
use ssmd_harman_ems::Ems;

use crate::positions::PositionsView;
use crate::reconciliation::ReconcileResult;

/// OMS metrics -- reconciliation and position-tracking counters.
/// EMS metrics (orders_dequeued, orders_submitted, etc.) are in EmsMetrics.
pub struct OmsMetrics {
    pub reconciliation_ok: prometheus::IntCounter,
    pub reconciliation_mismatch: prometheus::IntCounterVec,
    pub reconciliation_duration: prometheus::Histogram,
    pub reconciliation_last_success: prometheus::IntGauge,
    pub reconciliation_fills_discovered: prometheus::IntCounter,
    pub fills_external_imported: prometheus::IntCounter,
    pub reconciliation_unattributed_position: prometheus::IntCounter,
}

impl OmsMetrics {
    pub fn new(registry: &prometheus::Registry) -> Self {
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
        let fills_external_imported = prometheus::IntCounter::new(
            "harman_fills_external_imported_total",
            "External fills imported as synthetic orders",
        )
        .unwrap();
        let reconciliation_unattributed_position = prometheus::IntCounter::new(
            "harman_reconciliation_unattributed_position_total",
            "Position mismatches with exchange qty but zero local fills",
        )
        .unwrap();

        registry.register(Box::new(reconciliation_ok.clone())).unwrap();
        registry.register(Box::new(reconciliation_mismatch.clone())).unwrap();
        registry.register(Box::new(reconciliation_duration.clone())).unwrap();
        registry.register(Box::new(reconciliation_last_success.clone())).unwrap();
        registry.register(Box::new(reconciliation_fills_discovered.clone())).unwrap();
        registry.register(Box::new(fills_external_imported.clone())).unwrap();
        registry.register(Box::new(reconciliation_unattributed_position.clone())).unwrap();

        Self {
            reconciliation_ok,
            reconciliation_mismatch,
            reconciliation_duration,
            reconciliation_last_success,
            reconciliation_fills_discovered,
            fills_external_imported,
            reconciliation_unattributed_position,
        }
    }
}

/// The Order Management System.
///
/// Owns: reconciliation, recovery, positions, suspended sessions.
/// Delegates execution to the EMS.
pub struct Oms {
    pub pool: Pool,
    pub exchange: Arc<dyn ExchangeAdapter>,
    pub ems: Arc<Ems>,
    pub metrics: OmsMetrics,
    pub suspended_sessions: DashMap<i64, ()>,
    /// Session for external fills/orders not attributed to any user session
    pub system_session_id: i64,
}

impl Oms {
    pub fn new(
        pool: Pool,
        exchange: Arc<dyn ExchangeAdapter>,
        ems: Arc<Ems>,
        metrics: OmsMetrics,
        system_session_id: i64,
    ) -> Self {
        Self {
            pool,
            exchange,
            ems,
            metrics,
            suspended_sessions: DashMap::new(),
            system_session_id,
        }
    }

    pub fn is_suspended(&self, session_id: i64) -> bool {
        self.suspended_sessions.contains_key(&session_id)
    }

    pub fn resume(&self, session_id: i64) -> bool {
        self.suspended_sessions.remove(&session_id).is_some()
    }

    pub async fn reconcile(&self, session_id: i64) -> ReconcileResult {
        reconciliation::reconcile(self, session_id).await
    }

    pub async fn run_recovery(&self, session_id: i64) -> Result<(), String> {
        recovery::run(self, session_id).await
    }

    pub async fn positions(&self, session_id: i64) -> Result<PositionsView, String> {
        positions::positions(self, session_id).await
    }
}
