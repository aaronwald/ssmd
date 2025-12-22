use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::select;
use tracing::{error, info};

use crate::error::ConnectorError;
use crate::message::Message;
use crate::traits::{Connector, Writer};

/// Runner orchestrates the data collection pipeline
pub struct Runner<C: Connector, W: Writer> {
    feed_name: String,
    connector: C,
    writer: W,
    connected: Arc<AtomicBool>,
}

impl<C: Connector, W: Writer> Runner<C, W> {
    pub fn new(feed_name: impl Into<String>, connector: C, writer: W) -> Self {
        Self {
            feed_name: feed_name.into(),
            connector,
            writer,
            connected: Arc::new(AtomicBool::new(false)),
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
                        Some(data) => {
                            // Parse as JSON and wrap with metadata
                            let json_data = match serde_json::from_slice(&data) {
                                Ok(v) => v,
                                Err(_) => {
                                    // If not valid JSON, store as string
                                    serde_json::Value::String(
                                        String::from_utf8_lossy(&data).to_string()
                                    )
                                }
                            };

                            let message = Message::new(&self.feed_name, json_data);

                            if let Err(e) = self.writer.write(&message).await {
                                error!(error = %e, "Failed to write message");
                                // Continue on write errors
                            }
                        }
                        None => {
                            // Channel closed - connector disconnected
                            self.connected.store(false, Ordering::SeqCst);
                            info!("Connector disconnected");
                            break;
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
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::mpsc;

    struct MockConnector {
        #[allow(dead_code)]
        tx: mpsc::Sender<Vec<u8>>,
        rx: Option<mpsc::Receiver<Vec<u8>>>,
    }

    impl MockConnector {
        fn new() -> (Self, mpsc::Sender<Vec<u8>>) {
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
        fn messages(&mut self) -> mpsc::Receiver<Vec<u8>> {
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

        // Send a message
        msg_tx.send(b"{\"test\":true}".to_vec()).await.unwrap();

        // Wait a bit for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Shutdown
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap().unwrap();

        assert!(write_count.load(Ordering::SeqCst) >= 1);
    }
}
