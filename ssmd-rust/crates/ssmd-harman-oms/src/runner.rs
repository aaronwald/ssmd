use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use dashmap::DashSet;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::Oms;

/// Background task coordinator for auto-pump and auto-reconcile.
pub struct OmsRunner {
    oms: Arc<Oms>,
    pump_trigger: PumpTrigger,
    reconcile_interval: Option<Duration>,
    startup_session_id: i64,
    exchange_type: String,
    environment: String,
    shutdown: CancellationToken,
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
        exchange_type: String,
        environment: String,
    ) -> Self {
        Self {
            oms,
            pump_trigger: PumpTrigger::new(),
            reconcile_interval,
            startup_session_id,
            exchange_type,
            environment,
            shutdown: CancellationToken::new(),
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

    async fn auto_reconcile_loop(&self) {
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

            let session_ids = match harman::db::list_active_session_ids(
                &self.oms.pool,
                &self.exchange_type,
                &self.environment,
            )
            .await
            {
                Ok(ids) => ids,
                Err(e) => {
                    warn!(error = %e, "failed to list active sessions for reconciliation");
                    vec![self.startup_session_id]
                }
            };

            info!(sessions = session_ids.len(), "auto-reconcile starting");
            for sid in &session_ids {
                let result = self.oms.reconcile(*sid).await;
                if !result.errors.is_empty() {
                    warn!(session_id = sid, errors = ?result.errors, "reconciliation errors");
                }
            }
            info!(sessions = session_ids.len(), "auto-reconcile complete");
        }
    }
}
