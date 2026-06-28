//! Binance NATS Writer — publishes raw JSON messages to NATS
//!
//! Routes Binance combined-stream `@trade` frames to the appropriate NATS
//! subject and passes through the **raw bytes verbatim** — no transformation.
//!
//! Raw-publish choice: the connector forwards (and this writer publishes) the
//! **whole combined-stream frame** exactly as it arrived from Binance —
//! `{"stream":"btcusdt@trade","data":{...raw trade...}}` — mirroring the Kraken
//! writer's "publish what arrived" contract (Kraken publishes the entire
//! `{"channel":"trade",...,"data":[...]}` frame). The symbol is read from the
//! inner `data.s` (canonical upper-case, e.g. `BTCUSDT`), not the lower-case
//! `stream` token. Downstream parsers (`ssmd-bar-cache`, `ssmd-schemas`) read
//! the raw trade keys `s`/`p`/`q`/`T`/`t` under the `data` wrapper.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{trace, warn};

use ssmd_middleware::{sanitize_subject_token, SubjectBuilder, Transport};

use crate::error::WriterError;
use crate::message::Message;
use crate::traits::Writer;

/// Writer that publishes raw Binance JSON messages to NATS.
pub struct BinanceNatsWriter {
    transport: Arc<dyn Transport>,
    subjects: SubjectBuilder,
    message_count: u64,
}

impl BinanceNatsWriter {
    /// Create a new `BinanceNatsWriter` with default subject prefix:
    /// `{env_name}.{feed_name}`.
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl Into<Arc<str>>,
        feed_name: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::new(env_name, feed_name),
            message_count: 0,
        }
    }

    /// Create a new `BinanceNatsWriter` with a custom subject prefix and stream
    /// name (e.g. prefix `prod.binance.spot`, stream `PROD_BINANCE_SPOT`).
    pub fn with_prefix(
        transport: Arc<dyn Transport>,
        subject_prefix: impl Into<Arc<str>>,
        stream_name: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            transport,
            subjects: SubjectBuilder::with_prefix(subject_prefix, stream_name),
            message_count: 0,
        }
    }

    /// Get count of published messages.
    pub fn message_count(&self) -> u64 {
        self.message_count
    }
}

/// Partial view of a Binance combined-stream frame for fast-path routing.
///
/// Only `data.e` (event type) and `data.s` (symbol) are extracted, using
/// borrowed strings to avoid the untagged-enum / `serde_json::Value` overhead.
/// Command-response (`{"result":...,"id":N}`) and error (`{"error":{...}}`)
/// frames have no `data` and deserialize with `data: None`.
#[derive(Deserialize)]
struct PartialBinanceMsg<'a> {
    #[serde(borrow, default)]
    data: Option<PartialBinanceData<'a>>,
}

#[derive(Deserialize)]
struct PartialBinanceData<'a> {
    #[serde(rename = "e", default)]
    event_type: Option<&'a str>,
    #[serde(rename = "s", default)]
    symbol: Option<&'a str>,
}

#[async_trait]
impl Writer for BinanceNatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        // FAST PATH: parse just enough to extract the inner event type + symbol.
        let partial: PartialBinanceMsg = match serde_json::from_slice(&msg.data) {
            Ok(m) => m,
            Err(e) => {
                let preview: String = String::from_utf8_lossy(&msg.data)
                    .chars()
                    .take(500)
                    .collect();
                return Err(WriterError::WriteFailed(format!(
                    "Failed to parse Binance message: {}. Preview: {}",
                    e, preview
                )));
            }
        };

        // Non-data frames (command results, errors) carry no `data` — skip.
        let Some(data) = partial.data else {
            trace!("Skipping Binance frame with no data object");
            return Ok(());
        };

        // Only `@trade` events are forwarded; defend against any other event
        // type leaking through (we subscribe @trade-only, so this is belt-and-
        // braces).
        match data.event_type {
            Some("trade") => {}
            other => {
                trace!(event = ?other, "Skipping non-trade Binance frame");
                return Ok(());
            }
        }

        let symbol = data.symbol.unwrap_or("");
        let sanitized = sanitize_subject_token(symbol);
        if sanitized.is_empty() {
            warn!(raw_symbol = %symbol, "Empty sanitized Binance symbol, skipping");
            return Ok(());
        }

        let subject = self.subjects.json_trade(&sanitized);

        // Publish raw bytes — the whole combined-stream frame, no transformation.
        self.transport
            .publish(&subject, msg.data.clone())
            .await
            .map_err(|e| WriterError::WriteFailed(format!("NATS publish failed: {}", e)))?;

        self.message_count += 1;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), WriterError> {
        trace!(messages = self.message_count, "BinanceNatsWriter closing");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_middleware::InMemoryTransport;

    const TRADE_FRAME: &[u8] = br#"{"stream":"btcusdt@trade","data":{"e":"trade","E":1719580800000,"s":"BTCUSDT","t":123456,"p":"61234.50","q":"0.00100000","T":1719580799999,"m":false,"M":true}}"#;
    const FAN_TOKEN_FRAME: &[u8] = br#"{"stream":"psgusdt@trade","data":{"e":"trade","E":1719580800001,"s":"PSGUSDT","t":42,"p":"2.345","q":"10.0","T":1719580800000,"m":true,"M":true}}"#;

    #[tokio::test]
    async fn publishes_trade_frame_raw_and_whole() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = BinanceNatsWriter::new(transport.clone(), "dev", "binance");

        let mut sub = transport
            .subscribe("dev.binance.json.trade.BTCUSDT")
            .await
            .unwrap();

        let msg = Message::new("binance", TRADE_FRAME.to_vec());
        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "dev.binance.json.trade.BTCUSDT");
        // Raw passthrough: the whole combined-stream frame, byte-for-byte.
        assert_eq!(received.payload.as_ref(), TRADE_FRAME);
        assert_eq!(writer.message_count(), 1);
    }

    #[tokio::test]
    async fn routes_subject_from_inner_symbol() {
        // Subject must derive from `data.s` (upper-case canonical), proving we
        // route from the inner symbol, not the lower-case `stream` token.
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer =
            BinanceNatsWriter::with_prefix(transport.clone(), "prod.binance.spot", "PROD_BINANCE_SPOT");

        let mut sub = transport
            .subscribe("prod.binance.spot.json.trade.PSGUSDT")
            .await
            .unwrap();

        let msg = Message::new("binance", FAN_TOKEN_FRAME.to_vec());
        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "prod.binance.spot.json.trade.PSGUSDT");
        assert_eq!(writer.message_count(), 1);
    }

    #[tokio::test]
    async fn publishes_multiple_symbols_to_distinct_subjects() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer =
            BinanceNatsWriter::with_prefix(transport.clone(), "prod.binance.spot", "PROD_BINANCE_SPOT");

        let mut btc = transport
            .subscribe("prod.binance.spot.json.trade.BTCUSDT")
            .await
            .unwrap();
        let mut psg = transport
            .subscribe("prod.binance.spot.json.trade.PSGUSDT")
            .await
            .unwrap();

        writer
            .write(&Message::new("binance", TRADE_FRAME.to_vec()))
            .await
            .unwrap();
        writer
            .write(&Message::new("binance", FAN_TOKEN_FRAME.to_vec()))
            .await
            .unwrap();

        let btc_msg = btc.next().await.unwrap();
        assert_eq!(btc_msg.subject, "prod.binance.spot.json.trade.BTCUSDT");
        assert!(std::str::from_utf8(&btc_msg.payload).unwrap().contains("BTCUSDT"));

        let psg_msg = psg.next().await.unwrap();
        assert_eq!(psg_msg.subject, "prod.binance.spot.json.trade.PSGUSDT");
        assert!(std::str::from_utf8(&psg_msg.payload).unwrap().contains("PSGUSDT"));

        assert_eq!(writer.message_count(), 2);
    }

    #[tokio::test]
    async fn skips_command_result_frame() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = BinanceNatsWriter::new(transport.clone(), "dev", "binance");

        let frame = br#"{"result":null,"id":1}"#;
        let msg = Message::new("binance", frame.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn skips_error_frame() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = BinanceNatsWriter::new(transport.clone(), "dev", "binance");

        let frame = br#"{"error":{"code":2,"msg":"Invalid request"}}"#;
        let msg = Message::new("binance", frame.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn skips_non_trade_data_frame() {
        // A combined frame with a non-trade event type must not be published.
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = BinanceNatsWriter::new(transport.clone(), "dev", "binance");

        let frame = br#"{"stream":"btcusdt@kline_1m","data":{"e":"kline","s":"BTCUSDT","k":{}}}"#;
        let msg = Message::new("binance", frame.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn skips_trade_with_empty_symbol() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = BinanceNatsWriter::new(transport.clone(), "dev", "binance");

        let frame = br#"{"stream":"@trade","data":{"e":"trade","s":"","t":1,"p":"1.0","q":"1.0","T":1}}"#;
        let msg = Message::new("binance", frame.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn malformed_payload_is_error_not_panic() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = BinanceNatsWriter::new(transport.clone(), "dev", "binance");

        let msg = Message::new("binance", b"not json at all".to_vec());
        let result = writer.write(&msg).await;

        assert!(result.is_err());
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn message_count_increments_per_trade() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer = BinanceNatsWriter::new(transport.clone(), "dev", "binance");

        let _sub = transport
            .subscribe("dev.binance.json.trade.BTCUSDT")
            .await
            .unwrap();

        let msg = Message::new("binance", TRADE_FRAME.to_vec());
        writer.write(&msg).await.unwrap();
        writer.write(&msg).await.unwrap();

        assert_eq!(writer.message_count(), 2);
    }
}
