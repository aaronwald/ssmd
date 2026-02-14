//! Kraken Futures NATS writer — routes trade/ticker data to NATS subjects.
//!
//! Subject pattern: prod.kraken-futures.json.{feed}.{product_id}
//! e.g., prod.kraken-futures.json.trade.PI_XBTUSD

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, trace, warn};

use ssmd_middleware::{sanitize_subject_token, SubjectBuilder, Transport};

use crate::error::WriterError;
use crate::kraken_futures::messages::KrakenFuturesWsMessage;
use crate::message::Message;
use crate::traits::Writer;

pub struct KrakenFuturesNatsWriter {
    transport: Arc<dyn Transport>,
    subjects: SubjectBuilder,
    message_count: u64,
}

impl KrakenFuturesNatsWriter {
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<String>,
        feed_name: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::new(env_name, feed_name),
            message_count: 0,
        }
    }

    pub fn with_prefix(
        transport: Arc<dyn Transport>,
        subject_prefix: impl Into<String>,
        stream_name: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::with_prefix(subject_prefix, stream_name),
            message_count: 0,
        }
    }

    /// Get count of published messages
    pub fn message_count(&self) -> u64 {
        self.message_count
    }
}

#[async_trait]
impl Writer for KrakenFuturesNatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        let ws_msg: KrakenFuturesWsMessage = match serde_json::from_slice(&msg.data) {
            Ok(m) => m,
            Err(e) => {
                let preview: String = String::from_utf8_lossy(&msg.data)
                    .chars()
                    .take(500)
                    .collect();
                return Err(WriterError::WriteFailed(format!(
                    "Failed to parse Kraken Futures message: {}. Preview: {}",
                    e, preview
                )));
            }
        };

        let subject = match &ws_msg {
            KrakenFuturesWsMessage::DataMessage {
                feed, product_id, ..
            } => {
                let sanitized = sanitize_subject_token(product_id);
                if sanitized.is_empty() {
                    warn!(product_id = %product_id, "Empty sanitized product_id, skipping");
                    return Ok(());
                }
                match feed.as_str() {
                    "trade" | "trade_snapshot" => self.subjects.json_trade(&sanitized),
                    "ticker" => self.subjects.json_ticker(&sanitized),
                    _ => {
                        debug!(feed = %feed, "Unknown Kraken Futures feed, skipping");
                        return Ok(());
                    }
                }
            }
            // Skip non-data messages
            _ => return Ok(()),
        };

        // Publish raw bytes - no transformation
        self.transport
            .publish(&subject, msg.data.clone())
            .await
            .map_err(|e| WriterError::WriteFailed(format!("NATS publish failed: {}", e)))?;

        self.message_count += 1;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), WriterError> {
        trace!(count = self.message_count, "Kraken Futures writer closing");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_middleware::InMemoryTransport;

    #[test]
    fn test_sanitize_futures_symbol() {
        // Futures symbols like PI_XBTUSD, PF_ETHUSD should pass through
        assert_eq!(sanitize_subject_token("PI_XBTUSD"), "PI_XBTUSD");
        assert_eq!(sanitize_subject_token("PF_ETHUSD"), "PF_ETHUSD");
        assert_eq!(sanitize_subject_token("PF_XRPUSD"), "PF_XRPUSD");
    }

    #[tokio::test]
    async fn test_publish_trade_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenFuturesNatsWriter::new(transport.clone(), "dev", "kraken-futures");

        let mut sub = transport
            .subscribe("dev.kraken-futures.json.trade.PI_XBTUSD")
            .await
            .unwrap();

        let trade_json = br#"{"feed":"trade","product_id":"PI_XBTUSD","side":"buy","type":"fill","seq":12345,"time":1707300000000,"qty":0.001,"price":97000.0}"#;
        let msg = Message::new("kraken-futures", trade_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(
            received.subject,
            "dev.kraken-futures.json.trade.PI_XBTUSD"
        );
        assert_eq!(received.payload.as_ref(), trade_json);
    }

    #[tokio::test]
    async fn test_publish_ticker_json() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenFuturesNatsWriter::new(transport.clone(), "dev", "kraken-futures");

        let mut sub = transport
            .subscribe("dev.kraken-futures.json.ticker.PF_ETHUSD")
            .await
            .unwrap();

        let ticker_json = br#"{"feed":"ticker","product_id":"PF_ETHUSD","bid":3200.0,"ask":3201.0,"bid_size":10.0,"ask_size":5.0,"volume":50000.0,"dtm":0,"leverage":"50x","index":3200.5,"last":3200.0,"time":1707300000000,"change":50.0,"premium":0.1,"funding_rate":0.0001,"funding_rate_prediction":0.0001,"markPrice":3200.5,"openInterest":1000000.0}"#;
        let msg = Message::new("kraken-futures", ticker_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(
            received.subject,
            "dev.kraken-futures.json.ticker.PF_ETHUSD"
        );
        assert_eq!(received.payload.as_ref(), ticker_json);
    }

    #[tokio::test]
    async fn test_skip_heartbeat() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenFuturesNatsWriter::new(transport.clone(), "dev", "kraken-futures");

        let heartbeat_json = br#"{"event":"heartbeat"}"#;
        let msg = Message::new("kraken-futures", heartbeat_json.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_skip_subscribed() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenFuturesNatsWriter::new(transport.clone(), "dev", "kraken-futures");

        let sub_json =
            br#"{"event":"subscribed","feed":"trade","product_ids":["PI_XBTUSD","PF_ETHUSD"]}"#;
        let msg = Message::new("kraken-futures", sub_json.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn test_message_count() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenFuturesNatsWriter::new(transport.clone(), "dev", "kraken-futures");

        let _sub = transport
            .subscribe("dev.kraken-futures.json.trade.PI_XBTUSD")
            .await
            .unwrap();

        let trade_json = br#"{"feed":"trade","product_id":"PI_XBTUSD","side":"buy","type":"fill","seq":12345,"time":1707300000000,"qty":0.001,"price":97000.0}"#;
        let msg = Message::new("kraken-futures", trade_json.to_vec());

        writer.write(&msg).await.unwrap();
        writer.write(&msg).await.unwrap();

        assert_eq!(writer.message_count(), 2);
    }

    #[tokio::test]
    async fn test_with_prefix() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = KrakenFuturesNatsWriter::with_prefix(
            transport.clone(),
            "prod.kraken-futures",
            "PROD_KRAKEN_FUTURES",
        );

        let mut sub = transport
            .subscribe("prod.kraken-futures.json.ticker.PF_ETHUSD")
            .await
            .unwrap();

        let ticker_json = br#"{"feed":"ticker","product_id":"PF_ETHUSD","bid":3200.0,"ask":3201.0,"last":3200.0,"time":1707300000000}"#;
        let msg = Message::new("kraken-futures", ticker_json.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(
            received.subject,
            "prod.kraken-futures.json.ticker.PF_ETHUSD"
        );
    }
}
