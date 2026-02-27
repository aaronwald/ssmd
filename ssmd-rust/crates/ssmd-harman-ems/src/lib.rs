pub mod pump;
pub mod queue;
pub mod risk;
pub mod shutdown;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use deadpool_postgres::Pool;

use harman::exchange::ExchangeAdapter;
use harman::risk::RiskLimits;

use crate::pump::PumpResult;

/// EMS metrics -- execution-layer counters only.
/// Reconciliation metrics stay in the binary (will move to OMS later).
pub struct EmsMetrics {
    pub orders_dequeued: prometheus::IntCounter,
    pub orders_submitted: prometheus::IntCounter,
    pub orders_rejected: prometheus::IntCounter,
    pub orders_cancelled: prometheus::IntCounter,
    pub fills_recorded: prometheus::IntCounter,
    pub orders_amended: prometheus::IntCounter,
    pub orders_decreased: prometheus::IntCounter,
}

impl EmsMetrics {
    pub fn new(registry: &prometheus::Registry) -> Self {
        let orders_dequeued =
            prometheus::IntCounter::new("harman_orders_dequeued_total", "Orders dequeued from queue")
                .unwrap();
        let orders_submitted = prometheus::IntCounter::new(
            "harman_orders_submitted_total",
            "Orders submitted to exchange",
        )
        .unwrap();
        let orders_rejected = prometheus::IntCounter::new(
            "harman_orders_rejected_total",
            "Orders rejected by exchange",
        )
        .unwrap();
        let orders_cancelled =
            prometheus::IntCounter::new("harman_orders_cancelled_total", "Orders cancelled")
                .unwrap();
        let fills_recorded =
            prometheus::IntCounter::new("harman_fills_recorded_total", "Fills recorded").unwrap();
        let orders_amended = prometheus::IntCounter::new(
            "harman_orders_amended_total",
            "Orders amended on exchange",
        )
        .unwrap();
        let orders_decreased = prometheus::IntCounter::new(
            "harman_orders_decreased_total",
            "Orders decreased on exchange",
        )
        .unwrap();

        registry
            .register(Box::new(orders_dequeued.clone()))
            .unwrap();
        registry
            .register(Box::new(orders_submitted.clone()))
            .unwrap();
        registry
            .register(Box::new(orders_rejected.clone()))
            .unwrap();
        registry
            .register(Box::new(orders_cancelled.clone()))
            .unwrap();
        registry
            .register(Box::new(fills_recorded.clone()))
            .unwrap();
        registry
            .register(Box::new(orders_amended.clone()))
            .unwrap();
        registry
            .register(Box::new(orders_decreased.clone()))
            .unwrap();

        Self {
            orders_dequeued,
            orders_submitted,
            orders_rejected,
            orders_cancelled,
            fills_recorded,
            orders_amended,
            orders_decreased,
        }
    }
}

/// The Execution Management System.
///
/// Owns: queue processing (pump), order enqueue, execution-level risk checks,
/// graceful shutdown (mass cancel + drain). Does NOT own reconciliation,
/// recovery, positions, or auth -- those stay in the binary (future OMS).
pub struct Ems {
    pub pool: Pool,
    pub exchange: Arc<dyn ExchangeAdapter>,
    pub risk_limits: RiskLimits,
    pub metrics: EmsMetrics,
    pub shutting_down: AtomicBool,
}

impl Ems {
    pub fn new(
        pool: Pool,
        exchange: Arc<dyn ExchangeAdapter>,
        risk_limits: RiskLimits,
        metrics: EmsMetrics,
    ) -> Self {
        Self {
            pool,
            exchange,
            risk_limits,
            metrics,
            shutting_down: AtomicBool::new(false),
        }
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Relaxed)
    }

    pub async fn pump(&self, session_id: i64) -> PumpResult {
        pump::pump(self, session_id).await
    }

    pub async fn shutdown(&self) {
        shutdown::shutdown(self).await
    }
}
