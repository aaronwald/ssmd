use deadpool_postgres::Pool;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, info};
use uuid::Uuid;

#[derive(Debug)]
pub struct AuditEvent {
    pub event_id: Uuid,
    pub session_id: Option<i64>,
    pub order_id: Option<i64>,
    pub category: &'static str,
    pub action: String,
    pub endpoint: Option<String>,
    pub status_code: Option<i32>,
    pub duration_ms: Option<i32>,
    pub request: Option<Value>,
    pub response: Option<Value>,
    pub outcome: &'static str,
    pub error_msg: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Clone)]
pub struct AuditSender {
    tx: mpsc::Sender<AuditEvent>,
}

impl AuditSender {
    pub fn new(tx: mpsc::Sender<AuditEvent>) -> Self {
        Self { tx }
    }

    /// Fire-and-forget. Never blocks the caller.
    /// Crashes the process if the channel is full — don't silently drop audit data.
    pub fn send(&self, event: AuditEvent) {
        if self.tx.try_send(event).is_err() {
            error!("audit channel full — crashing pod (don't silently drop audit data)");
            std::process::exit(1);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rest_call(
        &self,
        session_id: i64,
        order_id: Option<i64>,
        action: &str,
        endpoint: &str,
        status_code: Option<i32>,
        duration_ms: Option<i32>,
        request: Option<Value>,
        response: Option<Value>,
        outcome: &'static str,
        error_msg: Option<String>,
    ) {
        self.send(AuditEvent {
            event_id: Uuid::new_v4(),
            session_id: Some(session_id),
            order_id,
            category: "rest_call",
            action: action.to_string(),
            endpoint: Some(endpoint.to_string()),
            status_code,
            duration_ms,
            request,
            response,
            outcome,
            error_msg,
            metadata: None,
        });
    }

    pub fn ws_event(
        &self,
        session_id: Option<i64>,
        order_id: Option<i64>,
        action: &str,
        request: Option<Value>,
        metadata: Option<Value>,
    ) {
        self.send(AuditEvent {
            event_id: Uuid::new_v4(),
            session_id,
            order_id,
            category: "ws_event",
            action: action.to_string(),
            endpoint: None,
            status_code: None,
            duration_ms: None,
            request,
            response: None,
            outcome: "success",
            error_msg: None,
            metadata,
        });
    }

    pub fn fallback(
        &self,
        session_id: i64,
        order_id: i64,
        action: &str,
        outcome: &'static str,
        metadata: Option<Value>,
    ) {
        self.send(AuditEvent {
            event_id: Uuid::new_v4(),
            session_id: Some(session_id),
            order_id: Some(order_id),
            category: "fallback",
            action: action.to_string(),
            endpoint: None,
            status_code: None,
            duration_ms: None,
            request: None,
            response: None,
            outcome,
            error_msg: None,
            metadata,
        });
    }

    pub fn reconciliation(
        &self,
        session_id: i64,
        order_id: Option<i64>,
        action: &str,
        outcome: &'static str,
        metadata: Option<Value>,
    ) {
        self.send(AuditEvent {
            event_id: Uuid::new_v4(),
            session_id: Some(session_id),
            order_id,
            category: "reconciliation",
            action: action.to_string(),
            endpoint: None,
            status_code: None,
            duration_ms: None,
            request: None,
            response: None,
            outcome,
            error_msg: None,
            metadata,
        });
    }

    pub fn risk(
        &self,
        session_id: i64,
        action: &str,
        outcome: &'static str,
        metadata: Option<Value>,
    ) {
        self.send(AuditEvent {
            event_id: Uuid::new_v4(),
            session_id: Some(session_id),
            order_id: None,
            category: "risk",
            action: action.to_string(),
            endpoint: None,
            status_code: None,
            duration_ms: None,
            request: None,
            response: None,
            outcome,
            error_msg: None,
            metadata,
        });
    }
}

pub struct AuditWriter {
    rx: mpsc::Receiver<AuditEvent>,
    pool: Pool,
    batch_size: usize,
    flush_interval: Duration,
}

impl AuditWriter {
    pub fn new(rx: mpsc::Receiver<AuditEvent>, pool: Pool) -> Self {
        Self {
            rx,
            pool,
            batch_size: 100,
            flush_interval: Duration::from_millis(500),
        }
    }

    pub async fn run(mut self) {
        let mut batch: Vec<AuditEvent> = Vec::with_capacity(self.batch_size);
        let mut ticker = interval(self.flush_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                Some(event) = self.rx.recv() => {
                    batch.push(event);
                    if batch.len() >= self.batch_size {
                        self.flush(&mut batch).await;
                    }
                }
                _ = ticker.tick() => {
                    if !batch.is_empty() {
                        self.flush(&mut batch).await;
                    }
                }
                else => break,
            }
        }

        // Drain remaining on shutdown
        if !batch.is_empty() {
            info!(count = batch.len(), "draining remaining audit events on shutdown");
            self.flush(&mut batch).await;
        }
    }

    async fn flush(&self, batch: &mut Vec<AuditEvent>) {
        let events: Vec<AuditEvent> = std::mem::take(batch);
        let count = events.len();
        match crate::db::batch_insert_audit(&self.pool, &events).await {
            Ok(n) => {
                debug!(count = n, "flushed audit events");
            }
            Err(e) => {
                error!(count, error = %e, "failed to flush audit events, retrying once");
                tokio::time::sleep(Duration::from_secs(1)).await;
                if let Err(e2) = crate::db::batch_insert_audit(&self.pool, &events).await {
                    error!(count, error = %e2, "retry failed — crashing pod (don't silently drop audit data)");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Create an audit channel pair. Returns (sender, writer).
pub fn create_audit_channel(pool: Pool) -> (AuditSender, AuditWriter) {
    let (tx, rx) = mpsc::channel(1024);
    (AuditSender::new(tx), AuditWriter::new(rx, pool))
}
