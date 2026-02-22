use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::select;
use tracing::{error, info};

use crate::error::ConnectorError;
use crate::message::Message;
use crate::metrics;
use crate::traits::{Connector, Writer};
use ssmd_middleware::{now_tsc, CLOCK};

/// Runner orchestrates the data collection pipeline
pub struct Runner<C: Connector, W: Writer> {
    feed_name: String,
    connector: C,
    writer: W,
    connected: Arc<AtomicBool>,
    /// Unix timestamp (seconds) of last message received
    last_message_epoch_secs: Arc<AtomicU64>,
}

impl<C: Connector, W: Writer> Runner<C, W> {
    pub fn new(feed_name: impl Into<String>, connector: C, writer: W) -> Self {
        Self {
            feed_name: feed_name.into(),
            connector,
            writer,
            connected: Arc::new(AtomicBool::new(false)),
            last_message_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Returns whether the connector is currently connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Returns a handle to the connected status
    pub fn connected_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.connected)
    }

    /// Returns a handle to the last message timestamp
    pub fn last_message_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.last_message_epoch_secs)
    }

    /// Returns the connector's activity handle if available.
    /// This tracks WebSocket activity (ping/pong + data messages) for health checks.
    /// Falls back to Runner's last_message_epoch_secs if connector doesn't track activity.
    pub fn activity_handle(&self) -> Arc<AtomicU64> {
        self.connector
            .activity_handle()
            .unwrap_or_else(|| Arc::clone(&self.last_message_epoch_secs))
    }

    /// Update last message timestamp to current time
    fn update_last_message_time(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.last_message_epoch_secs.store(now, Ordering::SeqCst);
    }

    /// Run the collection pipeline until cancelled or disconnected
    pub async fn run(&mut self, shutdown: tokio::sync::watch::Receiver<bool>) -> Result<(), ConnectorError> {
        // Connect
        self.connector.connect().await?;
        self.connected.store(true, Ordering::SeqCst);
        info!(feed = %self.feed_name, "Connected to data source");

        let mut rx = self.connector.messages();
        let mut shutdown = shutdown;

        loop {
            select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Shutdown signal received");
                        break;
                    }
                }
                msg = rx.recv() => {
                    match msg {
                        Some((ws_tsc, data)) => {
                            // Pass raw bytes through - no JSON parsing in hot path.
                            // Parsing/validation happens at I/O boundary (flusher, gateway).
                            let message = Message::new(&self.feed_name, data);
                            let write_start = now_tsc();

                            if let Err(e) = self.writer.write(&message).await {
                                // Write failures are fatal - indicates parse bug or NATS issue
                                error!(error = %e, "Failed to write message - exiting to trigger restart");
                                return Err(ConnectorError::WriteFailed(e.to_string()));
                            }
                            let write_end = now_tsc();
                            metrics::observe_nats_publish_duration(
                                &self.feed_name,
                                CLOCK.delta(write_start, write_end).as_secs_f64(),
                            );
                            // ws_tsc captured at WS receive in connector shard
                            metrics::observe_ws_process_duration(
                                &self.feed_name,
                                CLOCK.delta(ws_tsc, write_end).as_secs_f64(),
                            );
                            // Update last message time on successful write
                            self.update_last_message_time();
                        }
                        None => {
                            // Channel closed - connector disconnected unexpectedly
                            self.connected.store(false, Ordering::SeqCst);
                            error!("Connector disconnected unexpectedly - exiting to trigger restart");
                            return Err(ConnectorError::Disconnected("channel closed".to_string()));
                        }
                    }
                }
            }
        }

        // Cleanup
        self.connected.store(false, Ordering::SeqCst);
        self.writer.close().await.ok();
        self.connector.close().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::TimestampedMsg;
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::mpsc;

    struct MockConnector {
        #[allow(dead_code)]
        tx: mpsc::Sender<TimestampedMsg>,
        rx: Option<mpsc::Receiver<TimestampedMsg>>,
    }

    impl MockConnector {
        fn new() -> (Self, mpsc::Sender<TimestampedMsg>) {
            let (tx, rx) = mpsc::channel(10);
            let tx_clone = tx.clone();
            (
                Self {
                    tx,
                    rx: Some(rx),
                },
                tx_clone,
            )
        }
    }

    #[async_trait]
    impl Connector for MockConnector {
        async fn connect(&mut self) -> Result<(), ConnectorError> {
            Ok(())
        }
        fn messages(&mut self) -> mpsc::Receiver<TimestampedMsg> {
            self.rx.take().unwrap()
        }
        async fn close(&mut self) -> Result<(), ConnectorError> {
            Ok(())
        }
    }

    struct MockWriter {
        write_count: Arc<AtomicUsize>,
    }

    impl MockWriter {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let count = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    write_count: Arc::clone(&count),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl Writer for MockWriter {
        async fn write(&mut self, _msg: &Message) -> Result<(), crate::error::WriterError> {
            self.write_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn close(&mut self) -> Result<(), crate::error::WriterError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_runner_processes_messages() {
        let (connector, msg_tx) = MockConnector::new();
        let (writer, write_count) = MockWriter::new();

        let mut runner = Runner::new("test-feed", connector, writer);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Spawn runner
        let handle = tokio::spawn(async move {
            runner.run(shutdown_rx).await
        });

        // Send a message with TSC timestamp
        msg_tx.send((now_tsc(), b"{\"test\":true}".to_vec())).await.unwrap();

        // Wait a bit for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Shutdown
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap().unwrap();

        assert!(write_count.load(Ordering::SeqCst) >= 1);
    }
}
