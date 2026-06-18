//! Polygon.io ("massive") NATS writer
//!
//! Routes parsed Polygon trade and quote events to NATS subjects:
//!   Trade  → `{prefix}.json.trade.{sym}`
//!   Quote  → `{prefix}.json.quote.{sym}`
//!
//! Status and other event types are silently skipped — they are control
//! messages, not market data.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{trace, warn};

use ssmd_middleware::{sanitize_subject_token, Transport};

use crate::error::WriterError;
use crate::message::Message;
use crate::massive::messages::{split_frame_events, EventKind};
use crate::traits::Writer;

/// Subject builder for Polygon.io equity data.
///
/// Produces:
/// - `{prefix}.json.trade.{sym}` for trades
/// - `{prefix}.json.quote.{sym}` for quotes
pub struct MassiveSubjects {
    prefix: String,
}

impl MassiveSubjects {
    /// Create a new `MassiveSubjects` with the given subject prefix
    /// (e.g. `"prod.massive"` → `"prod.massive.json.trade.AAPL"`).
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
        }
    }

    /// Build the trade subject for an already-sanitized token.
    ///
    /// The caller is responsible for sanitizing the token via
    /// [`sanitize_subject_token`] before passing it here.
    /// In debug builds a `debug_assert!` verifies the token is non-empty and
    /// unchanged by a second sanitization pass.
    pub fn trade(&self, token: &str) -> String {
        debug_assert!(
            !token.is_empty() && sanitize_subject_token(token) == token,
            "token must be pre-sanitized: {:?}",
            token
        );
        format!("{}.json.trade.{}", self.prefix, token)
    }

    /// Build the quote subject for an already-sanitized token.
    ///
    /// The caller is responsible for sanitizing the token via
    /// [`sanitize_subject_token`] before passing it here.
    /// In debug builds a `debug_assert!` verifies the token is non-empty and
    /// unchanged by a second sanitization pass.
    pub fn quote(&self, token: &str) -> String {
        debug_assert!(
            !token.is_empty() && sanitize_subject_token(token) == token,
            "token must be pre-sanitized: {:?}",
            token
        );
        format!("{}.json.quote.{}", self.prefix, token)
    }
}

/// Writer that publishes Polygon.io JSON events to NATS, one message per event.
///
/// Each Polygon WS frame is a JSON array of events. This writer splits the
/// frame via [`split_frame_events`] and publishes **each individual event as
/// its own single-object JSON NATS message** (`{"ev":"T",...}` — NOT the whole
/// array). This preserves the pipeline contract: archiver injects
/// `_nats_seq`/`_received_at` once per NATS message, and `parse_batch`
/// deserialises each payload as exactly one JSON object → one parquet row.
pub struct MassiveNatsWriter {
    transport: Arc<dyn Transport>,
    subjects: MassiveSubjects,
    message_count: u64,
}

impl MassiveNatsWriter {
    /// Create a new `MassiveNatsWriter` with default subject prefix derived
    /// from `env_name` and `feed_name`.
    ///
    /// Follows the same naming convention as the Kraken/Polymarket peers:
    /// - subject prefix: `{env_name}.{feed_name}`
    /// - NATS stream name: `{ENV_NAME}_{FEED_NAME}` (uppercased, `_`-joined)
    ///
    /// # Panics
    /// Panics if `env_name` or `feed_name` are empty — an empty name would
    /// produce an invalid NATS subject prefix or stream name, which is an
    /// unrecoverable misconfiguration (crash loud, per architectural rules).
    pub fn new(
        transport: Arc<dyn Transport>,
        env_name: impl AsRef<str>,
        feed_name: impl AsRef<str>,
    ) -> Self {
        let env = env_name.as_ref();
        let feed = feed_name.as_ref();
        assert!(!env.is_empty(), "env_name must not be empty");
        assert!(!feed.is_empty(), "feed_name must not be empty");
        let prefix = format!("{}.{}", env, feed);
        let stream = format!("{}_{}", env.to_uppercase(), feed.to_uppercase());
        Self::with_prefix(transport, prefix, stream)
    }

    /// Create a new `MassiveNatsWriter` with an explicit subject prefix and
    /// NATS stream name.
    pub fn with_prefix(
        transport: Arc<dyn Transport>,
        subject_prefix: impl AsRef<str>,
        _stream_name: impl AsRef<str>,
    ) -> Self {
        Self {
            transport,
            subjects: MassiveSubjects::new(subject_prefix.as_ref()),
            message_count: 0,
        }
    }

    /// Return the total number of messages published to NATS.
    pub fn message_count(&self) -> u64 {
        self.message_count
    }
}

#[async_trait]
impl Writer for MassiveNatsWriter {
    async fn write(&mut self, msg: &Message) -> Result<(), WriterError> {
        // split_frame_events parses, sanitizes symbols, and drops empty-symbol
        // or malformed elements — never returns an event with an empty symbol.
        let events = split_frame_events(&msg.data);

        for event in events {
            // Defensive: split_frame_events guarantees non-empty symbols, but
            // guard here too so a future refactor can't produce a broken subject.
            if event.symbol.is_empty() {
                warn!("split_frame_events returned an event with an empty symbol — skipping");
                continue;
            }
            // Defensive: payload must be a valid non-empty NATS message.
            if event.payload.is_empty() {
                warn!(sym = %event.symbol, "split_frame_events returned an empty payload — skipping");
                continue;
            }

            let subject = match event.kind {
                EventKind::Trade => self.subjects.trade(&event.symbol),
                EventKind::Quote => self.subjects.quote(&event.symbol),
            };

            trace!(subject = %subject, "Publishing Polygon event");

            self.transport
                .publish(&subject, event.payload.into())
                .await
                .map_err(|e| WriterError::WriteFailed(format!("NATS publish failed: {}", e)))?;

            self.message_count += 1;
        }

        Ok(())
    }

    async fn close(&mut self) -> Result<(), WriterError> {
        trace!(messages = self.message_count, "MassiveNatsWriter closing");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_middleware::InMemoryTransport;

    // ── subject routing ──────────────────────────────────────────────────────

    #[test]
    fn subject_for_trade_and_quote() {
        let subjects = MassiveSubjects::new("prod.massive");
        assert_eq!(subjects.trade("AAPL"), "prod.massive.json.trade.AAPL");
        assert_eq!(subjects.quote("SPY"), "prod.massive.json.quote.SPY");
    }

    /// Verify that `MassiveNatsWriter::new` derives the same subject prefix as
    /// the Kraken peer: `{env}.{feed}` → subjects routed under that prefix.
    /// The stream derivation (`{ENV}_{FEED}`) is used by NATS JetStream publish
    /// but is not observable through `InMemoryTransport`, so we verify routing
    /// directly.
    #[tokio::test]
    async fn new_derives_prefix_from_env_and_feed() {
        let transport = Arc::new(InMemoryTransport::new());
        // Mirrors KrakenNatsWriter::new(transport, "prod", "massive") convention.
        let mut writer = MassiveNatsWriter::new(transport.clone(), "prod", "massive");

        let mut sub = transport
            .subscribe("prod.massive.json.trade.AAPL")
            .await
            .unwrap();

        let frame = br#"[{"ev":"T","sym":"AAPL","p":189.42,"s":100,"t":1718658000123,"q":987}]"#;
        let msg = Message::new("massive", frame.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        // Subject prefix must be "prod.massive" (not empty or wrong)
        assert_eq!(received.subject, "prod.massive.json.trade.AAPL");
        assert_eq!(writer.message_count(), 1);
    }

    // ── writer integration ───────────────────────────────────────────────────

    #[tokio::test]
    async fn publishes_trade_event() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer =
            MassiveNatsWriter::with_prefix(transport.clone(), "prod.massive", "PROD_MASSIVE");

        let mut sub = transport
            .subscribe("prod.massive.json.trade.AAPL")
            .await
            .unwrap();

        let frame =
            br#"[{"ev":"T","sym":"AAPL","p":189.42,"s":100,"t":1718658000123,"q":987}]"#;
        let msg = Message::new("massive", frame.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "prod.massive.json.trade.AAPL");
        // Payload must be a single JSON object (not the whole array)
        assert!(
            received.payload.starts_with(b"{"),
            "trade payload must be a JSON object, got: {:?}",
            std::str::from_utf8(&received.payload)
        );
        let payload_str = std::str::from_utf8(&received.payload).unwrap();
        assert!(payload_str.contains("\"ev\":\"T\""));
        assert!(payload_str.contains("\"sym\":\"AAPL\""));
        assert_eq!(writer.message_count(), 1);
    }

    #[tokio::test]
    async fn publishes_quote_event() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer =
            MassiveNatsWriter::with_prefix(transport.clone(), "prod.massive", "PROD_MASSIVE");

        let mut sub = transport
            .subscribe("prod.massive.json.quote.SPY")
            .await
            .unwrap();

        let frame =
            br#"[{"ev":"Q","sym":"SPY","bp":543.10,"bs":2,"ap":543.12,"as":3,"t":1718658000456}]"#;
        let msg = Message::new("massive", frame.to_vec());

        writer.write(&msg).await.unwrap();

        let received = sub.next().await.unwrap();
        assert_eq!(received.subject, "prod.massive.json.quote.SPY");
        // Payload must be a single JSON object (not the whole array)
        assert!(
            received.payload.starts_with(b"{"),
            "quote payload must be a JSON object, got: {:?}",
            std::str::from_utf8(&received.payload)
        );
        let payload_str = std::str::from_utf8(&received.payload).unwrap();
        assert!(payload_str.contains("\"ev\":\"Q\""));
        assert!(payload_str.contains("\"sym\":\"SPY\""));
        assert_eq!(writer.message_count(), 1);
    }

    #[tokio::test]
    async fn skips_status_events() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer =
            MassiveNatsWriter::with_prefix(transport.clone(), "prod.massive", "PROD_MASSIVE");

        let frame = br#"[{"ev":"status","status":"auth_success","message":"authenticated"}]"#;
        let msg = Message::new("massive", frame.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn skips_other_events() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer =
            MassiveNatsWriter::with_prefix(transport.clone(), "prod.massive", "PROD_MASSIVE");

        let frame = br#"[{"ev":"AM","sym":"AAPL","o":1.0,"c":2.0}]"#;
        let msg = Message::new("massive", frame.to_vec());

        writer.write(&msg).await.unwrap();
        assert_eq!(writer.message_count(), 0);
    }

    #[tokio::test]
    async fn counts_multiple_events_in_one_frame() {
        let transport = Arc::new(InMemoryTransport::new());
        let mut writer =
            MassiveNatsWriter::with_prefix(transport.clone(), "prod.massive", "PROD_MASSIVE");

        // Subscribe to both subjects before publishing
        let mut sub_trade = transport
            .subscribe("prod.massive.json.trade.AAPL")
            .await
            .unwrap();
        let mut sub_quote = transport
            .subscribe("prod.massive.json.quote.SPY")
            .await
            .unwrap();

        // Frame with status (skipped) + trade + quote — mixed frame, the bug scenario
        let frame = br#"[{"ev":"status","status":"connected","message":"Connected successfully"},{"ev":"T","sym":"AAPL","p":189.42,"s":100,"t":1718658000123,"q":987},{"ev":"Q","sym":"SPY","bp":543.10,"bs":2,"ap":543.12,"as":3,"t":1718658000456}]"#;
        let msg = Message::new("massive", frame.to_vec());

        writer.write(&msg).await.unwrap();

        // status is skipped, trade + quote = exactly 2 published (not 3)
        assert_eq!(writer.message_count(), 2);

        // Trade message: subject correct AND payload is the single trade object only
        let trade_msg = sub_trade.next().await.unwrap();
        assert_eq!(trade_msg.subject, "prod.massive.json.trade.AAPL");
        // Payload must be single object, not array
        assert!(
            trade_msg.payload.starts_with(b"{"),
            "trade payload must start with '{{', not '['"
        );
        let trade_str = std::str::from_utf8(&trade_msg.payload).unwrap();
        assert!(trade_str.contains("\"ev\":\"T\""), "trade payload must contain ev:T");
        assert!(trade_str.contains("\"sym\":\"AAPL\""), "trade payload must contain AAPL");
        // Must NOT contain the quote's symbol — proves no cross-contamination
        assert!(
            !trade_str.contains("\"sym\":\"SPY\""),
            "trade payload must not contain SPY — frame must be split"
        );

        // Quote message: subject correct AND payload is the single quote object only
        let quote_msg = sub_quote.next().await.unwrap();
        assert_eq!(quote_msg.subject, "prod.massive.json.quote.SPY");
        assert!(
            quote_msg.payload.starts_with(b"{"),
            "quote payload must start with '{{', not '['"
        );
        let quote_str = std::str::from_utf8(&quote_msg.payload).unwrap();
        assert!(quote_str.contains("\"ev\":\"Q\""), "quote payload must contain ev:Q");
        assert!(quote_str.contains("\"sym\":\"SPY\""), "quote payload must contain SPY");
        // Must NOT contain the trade's symbol
        assert!(
            !quote_str.contains("\"sym\":\"AAPL\""),
            "quote payload must not contain AAPL — frame must be split"
        );
    }
}
