//! Polygon.io WebSocket message types
//!
//! Polygon WS frames are JSON arrays of events. Each event has an `ev` field
//! that identifies the event type: "T" for trades, "Q" for quotes, "status"
//! for connection/auth status messages.

use serde::Deserialize;

/// A trade event from Polygon.io (`"ev": "T"`).
///
/// Timestamps (`t`) are Unix milliseconds. `q` is a per-symbol sequence number.
#[derive(Debug, Clone, Deserialize)]
pub struct MassiveTrade {
    /// Ticker symbol (e.g. `"AAPL"`).
    pub sym: String,
    /// Trade price.
    pub p: f64,
    /// Trade size (shares).
    pub s: f64,
    /// Exchange timestamp in Unix milliseconds.
    pub t: i64,
    /// Per-symbol sequence number (Polygon `q` field).
    #[serde(default)]
    pub q: i64,
}

/// A quote (NBBO) event from Polygon.io (`"ev": "Q"`).
#[derive(Debug, Clone, Deserialize)]
pub struct MassiveQuote {
    /// Ticker symbol.
    pub sym: String,
    /// Bid price.
    pub bp: f64,
    /// Bid size.
    pub bs: f64,
    /// Ask price.
    pub ap: f64,
    /// Ask size. Renamed because `as` is a Rust keyword.
    #[serde(rename = "as")]
    pub as_: f64,
    /// Exchange timestamp in Unix milliseconds.
    pub t: i64,
}

/// A status/auth event from Polygon.io (`"ev": "status"`).
#[derive(Debug, Clone, Deserialize)]
pub struct MassiveStatus {
    /// Status string, e.g. `"auth_success"`, `"connected"`.
    pub status: String,
    /// Human-readable message.
    #[serde(default)]
    pub message: String,
}

/// Discriminated union of all Polygon WS event types the connector handles.
#[derive(Debug, Clone)]
pub enum MassiveMessage {
    Trade(MassiveTrade),
    Quote(MassiveQuote),
    Status(MassiveStatus),
    /// Any `ev` value not explicitly handled (e.g. `"AM"` aggregate minutes).
    Other,
}

/// Parse a raw Polygon WS frame (a JSON array of event objects) into a
/// `Vec<MassiveMessage>`.
///
/// Malformed frames and individual events that fail to deserialise both
/// degrade to `Other` or an empty vec rather than propagating errors —
/// the connector must never crash on unexpected market data.
pub fn parse_frame(bytes: &[u8]) -> Vec<MassiveMessage> {
    let values: Vec<serde_json::Value> = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    values
        .into_iter()
        .map(|v| match v.get("ev").and_then(|e| e.as_str()) {
            Some("T") => serde_json::from_value(v)
                .map(MassiveMessage::Trade)
                .unwrap_or(MassiveMessage::Other),
            Some("Q") => serde_json::from_value(v)
                .map(MassiveMessage::Quote)
                .unwrap_or(MassiveMessage::Other),
            Some("status") => serde_json::from_value(v)
                .map(MassiveMessage::Status)
                .unwrap_or(MassiveMessage::Other),
            _ => MassiveMessage::Other,
        })
        .collect()
}

/// The kind of a split frame event (trade or quote).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Trade,
    Quote,
}

/// A single event extracted from a Polygon WS frame, carrying the raw
/// per-event JSON bytes so the archiver receives exactly one JSON object per
/// NATS message.
#[derive(Debug, Clone)]
pub struct MassiveEvent {
    pub kind: EventKind,
    pub symbol: String,
    /// Raw JSON bytes for this single event object (starts with `{`, not `[`).
    pub payload: Vec<u8>,
}

/// Split a raw Polygon WS frame (JSON array of event objects) into individual
/// [`MassiveEvent`]s.
///
/// - Only `"ev":"T"` (trade) and `"ev":"Q"` (quote) events are returned.
/// - `"ev":"status"` and all other event types are silently skipped.
/// - Malformed frames or elements with an empty / missing `sym` after sanitisation
///   are skipped rather than propagating errors.
/// - The `payload` field contains the compact JSON serialisation of the *single*
///   event object so that the archiver writes exactly one parquet row per NATS message.
pub fn split_frame_events(bytes: &[u8]) -> Vec<MassiveEvent> {
    use ssmd_middleware::sanitize_subject_token;

    let values: Vec<serde_json::Value> = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut events = Vec::new();
    for value in values {
        let ev = match value.get("ev").and_then(|e| e.as_str()) {
            Some(ev) => ev,
            None => continue,
        };
        let kind = match ev {
            "T" => EventKind::Trade,
            "Q" => EventKind::Quote,
            _ => continue, // status, AM, and anything else — skip
        };
        let sym_raw = match value.get("sym").and_then(|s| s.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let symbol = sanitize_subject_token(sym_raw);
        if symbol.is_empty() {
            tracing::warn!(sym = %sym_raw, "Empty sanitized symbol in split_frame_events, skipping");
            continue;
        }
        let payload = match serde_json::to_vec(&value) {
            Ok(p) => p,
            Err(_) => continue,
        };
        events.push(MassiveEvent {
            kind,
            symbol,
            payload,
        });
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trade_array_frame() {
        // Polygon sends arrays; "ev":"T" is a trade.
        let raw = br#"[{"ev":"T","sym":"AAPL","i":"12345","x":11,"p":189.42,"s":100,"c":[14],"t":1718658000123,"q":987,"z":3}]"#;
        let msgs = parse_frame(raw);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            MassiveMessage::Trade(t) => {
                assert_eq!(t.sym, "AAPL");
                assert_eq!(t.p, 189.42);
                assert_eq!(t.s, 100.0);
                assert_eq!(t.t, 1718658000123);
                assert_eq!(t.q, 987);
            }
            other => panic!("expected Trade, got {other:?}"),
        }
    }

    #[test]
    fn parses_quote_and_status_in_one_frame() {
        let raw = br#"[{"ev":"status","status":"auth_success","message":"authenticated"},{"ev":"Q","sym":"SPY","bp":543.10,"bs":2,"ap":543.12,"as":3,"t":1718658000456,"z":3}]"#;
        let msgs = parse_frame(raw);
        assert_eq!(msgs.len(), 2);
        assert!(matches!(&msgs[0], MassiveMessage::Status(s) if s.status == "auth_success"));
        match &msgs[1] {
            MassiveMessage::Quote(q) => {
                assert_eq!(q.sym, "SPY");
                assert_eq!(q.bp, 543.10);
                assert_eq!(q.ap, 543.12);
            }
            other => panic!("expected Quote, got {other:?}"),
        }
    }

    #[test]
    fn unknown_event_becomes_other_not_error() {
        let raw = br#"[{"ev":"AM","sym":"AAPL","o":1.0,"c":2.0}]"#;
        let msgs = parse_frame(raw);
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], MassiveMessage::Other));
    }

    // ── split_frame_events ───────────────────────────────────────────────────

    #[test]
    fn split_frame_events_extracts_trade_and_quote_skips_status() {
        let raw = br#"[{"ev":"status","status":"connected","message":"Connected"},{"ev":"T","sym":"AAPL","p":189.42,"s":100,"t":1718658000123,"q":987},{"ev":"Q","sym":"SPY","bp":543.10,"bs":2,"ap":543.12,"as":3,"t":1718658000456}]"#;
        let events = split_frame_events(raw);
        assert_eq!(events.len(), 2, "status must be skipped");

        // First event is the trade
        assert_eq!(events[0].kind, EventKind::Trade);
        assert_eq!(events[0].symbol, "AAPL");
        // payload is a single object (starts with '{', not '[')
        assert!(events[0].payload.starts_with(b"{"), "trade payload must be a JSON object");
        // payload must NOT contain the quote's sym
        let trade_str = std::str::from_utf8(&events[0].payload).unwrap();
        assert!(!trade_str.contains("\"SPY\""), "trade payload must not contain SPY");
        assert!(trade_str.contains("\"AAPL\""));
        assert!(trade_str.contains("\"ev\":\"T\""));

        // Second event is the quote
        assert_eq!(events[1].kind, EventKind::Quote);
        assert_eq!(events[1].symbol, "SPY");
        assert!(events[1].payload.starts_with(b"{"), "quote payload must be a JSON object");
        let quote_str = std::str::from_utf8(&events[1].payload).unwrap();
        assert!(!quote_str.contains("\"AAPL\""), "quote payload must not contain AAPL");
        assert!(quote_str.contains("\"SPY\""));
        assert!(quote_str.contains("\"ev\":\"Q\""));
    }

    #[test]
    fn split_frame_events_skips_unknown_ev() {
        let raw = br#"[{"ev":"AM","sym":"AAPL","o":1.0,"c":2.0}]"#;
        let events = split_frame_events(raw);
        assert!(events.is_empty(), "AM events must be skipped");
    }

    #[test]
    fn split_frame_events_malformed_bytes_returns_empty() {
        let events = split_frame_events(b"not json at all !!!");
        assert!(events.is_empty(), "malformed bytes must yield empty vec");
    }

    #[test]
    fn split_frame_events_preserves_extra_fields_in_payload() {
        // Polygon trades carry fields like x, c[], z that must survive in the payload
        let raw = br#"[{"ev":"T","sym":"AAPL","i":"12345","x":11,"p":189.42,"s":100,"c":[14],"t":1718658000123,"q":987,"z":3}]"#;
        let events = split_frame_events(raw);
        assert_eq!(events.len(), 1);
        let payload_str = std::str::from_utf8(&events[0].payload).unwrap();
        // Verify extra fields are preserved
        assert!(payload_str.contains("\"x\":"), "exchange id field must be preserved");
        assert!(payload_str.contains("\"z\":"), "tape field must be preserved");
        assert!(payload_str.contains("\"i\":"), "trade id field must be preserved");
    }
}
