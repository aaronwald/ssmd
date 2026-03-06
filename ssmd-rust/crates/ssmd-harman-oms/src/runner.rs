use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use dashmap::DashSet;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use harman::exchange::EventStream;

use crate::Oms;
use crate::event_ingester::EventIngester;

/// Background task coordinator for auto-pump, auto-reconcile, and WS event ingestion.
pub struct OmsRunner {
    oms: Arc<Oms>,
    pump_trigger: PumpTrigger,
    reconcile_interval: Option<Duration>,
    startup_session_id: i64,
    shutdown: CancellationToken,
    /// Optional WS event stream for real-time events.
    event_stream: Option<Arc<dyn EventStream>>,
}

/// Cheap, cloneable handle for REST handlers to trigger auto-pump.
/// Carries session_id so the runner knows which session to pump.
#[derive(Clone)]
pub struct PumpTrigger {
    dirty_sessions: Arc<DashSet<i64>>,
    notify: Arc<tokio::sync::Notify>,
}

impl PumpTrigger {
    pub fn new() -> Self {
        Self {
            dirty_sessions: Arc::new(DashSet::new()),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Signal that a session has pending queue items.
    pub fn notify(&self, session_id: i64) {
        self.dirty_sessions.insert(session_id);
        self.notify.notify_one();
    }
}

impl Default for PumpTrigger {
    fn default() -> Self {
        Self::new()
    }
}

impl OmsRunner {
    pub fn new(
        oms: Arc<Oms>,
        reconcile_interval: Option<Duration>,
        startup_session_id: i64,
        event_stream: Option<Arc<dyn EventStream>>,
    ) -> Self {
        Self {
            oms,
            pump_trigger: PumpTrigger::new(),
            reconcile_interval,
            startup_session_id,
            shutdown: CancellationToken::new(),
            event_stream,
        }
    }

    pub fn pump_trigger(&self) -> PumpTrigger {
        self.pump_trigger.clone()
    }

    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Run background tasks. Blocks until shutdown.
    pub async fn run(&self, session_semaphores: &DashMap<i64, Arc<Semaphore>>) {
        info!("OMS runner started");
        tokio::select! {
            () = self.auto_pump_loop(session_semaphores) => {}
            () = self.auto_reconcile_loop() => {}
            () = self.ws_event_loop() => {}
            () = self.shutdown.cancelled() => {
                info!("OMS runner shutting down");
            }
        }
    }

    async fn auto_pump_loop(&self, session_semaphores: &DashMap<i64, Arc<Semaphore>>) {
        loop {
            self.pump_trigger.notify.notified().await;
            // Coalesce rapid mutations into one pump
            tokio::time::sleep(Duration::from_millis(50)).await;

            let sessions: Vec<i64> = self
                .pump_trigger
                .dirty_sessions
                .iter()
                .map(|r| *r)
                .collect();
            for sid in sessions {
                self.pump_trigger.dirty_sessions.remove(&sid);

                // Respect per-session semaphore -- skip if manual pump running
                let sem = session_semaphores
                    .entry(sid)
                    .or_insert_with(|| Arc::new(Semaphore::new(1)))
                    .clone();
                if let Ok(_permit) = Arc::clone(&sem).try_acquire_owned() {
                    let result = self.oms.ems.pump(sid).await;
                    if !result.errors.is_empty() {
                        warn!(session_id = sid, errors = ?result.errors, "auto-pump errors");
                    }

                    // Evaluate group triggers after pump
                    match self.oms.evaluate_triggers(sid).await {
                        Ok(activated) if activated > 0 => {
                            info!(session_id = sid, activated, "triggers activated, re-pumping");
                            let result = self.oms.ems.pump(sid).await;
                            if !result.errors.is_empty() {
                                warn!(session_id = sid, errors = ?result.errors, "re-pump errors");
                            }
                        }
                        Ok(_) => {}
                        Err(e) => warn!(session_id = sid, error = %e, "trigger evaluation failed"),
                    }
                }
            }
        }
    }

    /// Reconcile the startup session on a configurable interval.
    ///
    /// When WS is enabled, reconciliation is disabled entirely — the WS event
    /// ingester handles all live state updates. Recovery on startup handles
    /// the cold-start gap.
    ///
    /// Without WS (REST-only mode), uses the configured `reconcile_interval`.
    async fn auto_reconcile_loop(&self) {
        // WS mode: no reconciliation — WS handles the live path
        if self.event_stream.is_some() {
            info!("WS enabled — reconciliation disabled");
            std::future::pending::<()>().await;
            return;
        }

        let interval = match self.reconcile_interval {
            Some(d) if d > Duration::ZERO => d,
            _ => {
                // Disabled -- park forever
                std::future::pending::<()>().await;
                return;
            }
        };

        loop {
            tokio::time::sleep(interval).await;

            info!(
                session_id = self.startup_session_id,
                interval_secs = interval.as_secs(),
                "auto-reconcile starting"
            );
            let result = self.oms.reconcile(self.startup_session_id).await;
            if !result.errors.is_empty() {
                warn!(
                    session_id = self.startup_session_id,
                    errors = ?result.errors,
                    "reconciliation errors"
                );
            }
            info!(session_id = self.startup_session_id, "auto-reconcile complete");
        }
    }

    /// Run the WS event ingester if an event stream is configured.
    /// Parks forever if no event stream is available.
    async fn ws_event_loop(&self) {
        let event_stream = match &self.event_stream {
            Some(es) => es,
            None => {
                // No WS -- park forever
                std::future::pending::<()>().await;
                return;
            }
        };

        let rx = event_stream.subscribe();
        let ingester = EventIngester::new(
            self.oms.pool.clone(),
            self.oms.exchange.clone(),
            self.oms.metrics.clone(),
            self.oms.audit.clone(),
            self.pump_trigger.clone(),
        );

        info!("WS event ingester started");
        let result = ingester.run(rx).await;

        info!(
            events_processed = result.events_processed,
            fills_recorded = result.fills_recorded,
            orders_updated = result.orders_updated,
            settlements_noted = result.settlements_noted,
            "WS event ingester stopped"
        );
    }
}
