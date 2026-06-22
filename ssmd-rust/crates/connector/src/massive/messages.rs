//! Polygon.io WebSocket message types
//!
//! Polygon WS frames are JSON arrays of events. Each event has an `ev` field
//! that identifies the event type: "A" for per-second aggregates, "AM" for
//! per-minute aggregates, "status" for connection/auth status messages.
//!
//! The account is on Polygon's Starter plan, which authorizes aggregate
//! channels (`A.`/`AM.`) but NOT trades (`T.`) or quotes (`Q.`). The Trade /
//! Quote types are retained for forward-compatibility but no longer flow.

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

/// An OHLCV aggregate event from Polygon.io.
///
/// Per-second aggregates arrive as `"ev": "A"`, per-minute aggregates as
/// `"ev": "AM"`. Both carry identical fields. `s`/`e` are the aggregate
/// window start/end in Unix milliseconds.
///
/// `vw` (window VWAP), `av` (accumulated daily volume), `a` (daily VWAP) and
/// `z` (average trade size) are `#[serde(default)]` so the connector tolerates
/// their absence rather than dropping an otherwise-valid bar.
///
/// This typed view is provided for callers that want a structured aggregate;
/// the NATS data path in [`split_frame_events`] routes the raw single-object
/// JSON payload (validated field-by-field downstream by `parse_batch`) so that
/// no field is silently lost in transit.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct MassiveAgg {
    /// Ticker symbol (e.g. `"AAPL"`).
    pub sym: String,
    /// Open price for this window.
    pub o: f64,
    /// High price for this window.
    pub h: f64,
    /// Low price for this window.
    pub l: f64,
    /// Close price for this window.
    pub c: f64,
    /// Volume traded during this window.
    pub v: f64,
    /// Volume-weighted average price for this window.
    #[serde(default)]
    pub vw: f64,
    /// Accumulated volume for the current trading day.
    #[serde(default)]
    pub av: f64,
    /// Volume-weighted average price for the current trading day.
    #[serde(default)]
    pub a: f64,
    /// Average trade size during this window.
    #[serde(default)]
    pub z: f64,
    /// Window start timestamp in Unix milliseconds.
    pub s: i64,
    /// Window end timestamp in Unix milliseconds.
    pub e: i64,
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
    /// Any `ev` value not explicitly handled (e.g. `"A"`/`"AM"` aggregates,
    /// which flow through [`split_frame_events`] on the data path instead).
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

/// The kind of a split frame event.
///
/// The Starter plan only authorizes aggregate channels, so the data path
/// emits [`EventKind::AggSecond`] (`"ev":"A"`) and [`EventKind::AggMinute`]
/// (`"ev":"AM"`). `Trade`/`Quote` are retained for forward-compatibility but
/// are no longer produced by [`split_frame_events`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Trade,
    Quote,
    /// Per-second OHLCV aggregate (`"ev":"A"`).
    AggSecond,
    /// Per-minute OHLCV aggregate (`"ev":"AM"`).
    AggMinute,
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
/// - Only `"ev":"A"` (per-second aggregate) and `"ev":"AM"` (per-minute
///   aggregate) events are returned — the channels the Starter plan authorizes.
/// - `"ev":"status"`, `"ev":"T"`, `"ev":"Q"`, and all other event types are
///   silently skipped.
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
            "A" => EventKind::AggSecond,
            "AM" => EventKind::AggMinute,
            _ => continue, // status, T, Q, and anything else — skip
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
    fn aggregate_events_become_other_in_parse_frame() {
        // parse_frame is only used by the auth handshake, which inspects Status;
        // A/AM aggregates are not handled there and degrade to Other.
        let raw = br#"[{"ev":"A","sym":"AAPL","o":1.0,"c":2.0,"h":2.0,"l":1.0,"v":10,"s":1,"e":2},{"ev":"AM","sym":"SPY","o":1.0,"c":2.0,"h":2.0,"l":1.0,"v":10,"s":1,"e":2}]"#;
        let msgs = parse_frame(raw);
        assert_eq!(msgs.len(), 2);
        assert!(matches!(msgs[0], MassiveMessage::Other));
        assert!(matches!(msgs[1], MassiveMessage::Other));
    }

    #[test]
    fn massive_agg_deserializes_with_integer_numbers() {
        // v, vw, z may arrive as integer JSON numbers — serde f64 handles both.
        let raw = br#"{"ev":"A","sym":"AAPL","v":80,"av":144673,"vw":296.32,"o":296.32,"c":296.32,"h":296.32,"l":296.32,"a":296.6776,"z":80,"s":1782124955000,"e":1782124956000}"#;
        let agg: MassiveAgg = serde_json::from_slice(raw).unwrap();
        assert_eq!(agg.sym, "AAPL");
        assert_eq!(agg.o, 296.32);
        assert_eq!(agg.v, 80.0);
        assert_eq!(agg.vw, 296.32);
        assert_eq!(agg.z, 80.0);
        assert_eq!(agg.s, 1782124955000);
        assert_eq!(agg.e, 1782124956000);
    }

    #[test]
    fn massive_agg_tolerates_missing_optional_fields() {
        // vw/av/a/z are #[serde(default)] — absence must default to 0.0, not error.
        let raw = br#"{"ev":"A","sym":"AAPL","o":1.0,"c":2.0,"h":2.0,"l":1.0,"v":10,"s":1,"e":2}"#;
        let agg: MassiveAgg = serde_json::from_slice(raw).unwrap();
        assert_eq!(agg.vw, 0.0);
        assert_eq!(agg.av, 0.0);
        assert_eq!(agg.a, 0.0);
        assert_eq!(agg.z, 0.0);
    }

    // ── split_frame_events ───────────────────────────────────────────────────

    #[test]
    fn split_frame_events_extracts_aggregates_skips_status() {
        let raw = br#"[{"ev":"status","status":"connected","message":"Connected"},{"ev":"A","sym":"AAPL","o":296.32,"c":296.32,"h":296.32,"l":296.32,"v":80,"vw":296.32,"s":1782124955000,"e":1782124956000},{"ev":"AM","sym":"SPY","o":543.10,"c":543.12,"h":543.20,"l":543.00,"v":2000,"vw":543.11,"s":1782124920000,"e":1782124980000}]"#;
        let events = split_frame_events(raw);
        assert_eq!(events.len(), 2, "status must be skipped");

        // First event is the per-second aggregate
        assert_eq!(events[0].kind, EventKind::AggSecond);
        assert_eq!(events[0].symbol, "AAPL");
        // payload is a single object (starts with '{', not '[')
        assert!(events[0].payload.starts_with(b"{"), "agg payload must be a JSON object");
        // payload must NOT contain the other symbol
        let a_str = std::str::from_utf8(&events[0].payload).unwrap();
        assert!(!a_str.contains("\"SPY\""), "A payload must not contain SPY");
        assert!(a_str.contains("\"AAPL\""));
        assert!(a_str.contains("\"ev\":\"A\""));

        // Second event is the per-minute aggregate
        assert_eq!(events[1].kind, EventKind::AggMinute);
        assert_eq!(events[1].symbol, "SPY");
        assert!(events[1].payload.starts_with(b"{"), "agg payload must be a JSON object");
        let am_str = std::str::from_utf8(&events[1].payload).unwrap();
        assert!(!am_str.contains("\"AAPL\""), "AM payload must not contain AAPL");
        assert!(am_str.contains("\"SPY\""));
        assert!(am_str.contains("\"ev\":\"AM\""));
    }

    #[test]
    fn split_frame_events_skips_trade_and_quote() {
        // T/Q no longer flow on the Starter plan — they must be skipped.
        let raw = br#"[{"ev":"T","sym":"AAPL","p":189.42,"s":100,"t":1718658000123,"q":987},{"ev":"Q","sym":"SPY","bp":543.10,"bs":2,"ap":543.12,"as":3,"t":1718658000456}]"#;
        let events = split_frame_events(raw);
        assert!(events.is_empty(), "T and Q events must be skipped on Starter plan");
    }

    #[test]
    fn split_frame_events_skips_status() {
        let raw = br#"[{"ev":"status","status":"auth_success","message":"authenticated"}]"#;
        let events = split_frame_events(raw);
        assert!(events.is_empty(), "status events must be skipped");
    }

    #[test]
    fn split_frame_events_malformed_bytes_returns_empty() {
        let events = split_frame_events(b"not json at all !!!");
        assert!(events.is_empty(), "malformed bytes must yield empty vec");
    }

    #[test]
    fn split_frame_events_preserves_extra_fields_in_payload() {
        // Polygon aggregates carry fields like av, a, z, dv, dav that must survive.
        let raw = br#"[{"ev":"A","sym":"AAPL","v":80,"av":144673,"vw":296.32,"o":296.32,"c":296.32,"h":296.32,"l":296.32,"a":296.6776,"z":80,"s":1782124955000,"e":1782124956000,"dv":"80.0","dav":"144673.243177"}]"#;
        let events = split_frame_events(raw);
        assert_eq!(events.len(), 1);
        let payload_str = std::str::from_utf8(&events[0].payload).unwrap();
        // Verify extra fields are preserved
        assert!(payload_str.contains("\"av\":"), "accumulated volume field must be preserved");
        assert!(payload_str.contains("\"a\":"), "daily vwap field must be preserved");
        assert!(payload_str.contains("\"z\":"), "avg trade size field must be preserved");
        assert!(payload_str.contains("\"dv\":"), "daily volume string field must be preserved");
    }

    #[test]
    fn split_frame_events_single_object_payload() {
        // A frame with two aggregates must yield two single-object payloads,
        // never the whole array.
        let raw = br#"[{"ev":"A","sym":"AAPL","o":1.0,"c":2.0,"h":2.0,"l":1.0,"v":10,"s":1,"e":2},{"ev":"A","sym":"MSFT","o":3.0,"c":4.0,"h":4.0,"l":3.0,"v":20,"s":1,"e":2}]"#;
        let events = split_frame_events(raw);
        assert_eq!(events.len(), 2);
        for ev in &events {
            assert!(ev.payload.starts_with(b"{"), "each payload must be a single object");
            assert!(!ev.payload.starts_with(b"["), "payload must not be an array");
        }
    }
}
