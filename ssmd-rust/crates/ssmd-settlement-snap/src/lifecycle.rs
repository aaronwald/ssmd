//! Lifecycle message parsing and settlement-trigger detection.
//!
//! Mirrors the `LifecycleMsg` shape used by `ssmd-cache`, but adds
//! `settlement_value` and a `result` accessor that falls back to
//! `additional_metadata.result` when the top-level field is absent.

use serde::Deserialize;

/// Outer NATS envelope: `{ "type": "market_lifecycle_v2", "msg": { ... } }`.
#[derive(Debug, Deserialize)]
pub struct RawLifecycleMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub msg: LifecycleMsg,
}

/// Inner lifecycle payload. All fields beyond the identifiers are optional
/// because different `event_type`s carry different subsets.
#[derive(Debug, Deserialize)]
pub struct LifecycleMsg {
    pub market_ticker: String,
    pub event_type: String,
    #[serde(default)]
    pub event_ticker: Option<String>,
    #[serde(default)]
    pub open_ts: Option<i64>,
    #[serde(default)]
    pub close_ts: Option<i64>,
    #[serde(default)]
    pub determination_ts: Option<i64>,
    #[serde(default)]
    pub settled_ts: Option<i64>,
    /// Top-level result, if present. Use [`LifecycleMsg::result`] which also
    /// checks `additional_metadata`.
    #[serde(default)]
    pub result: Option<String>,
    /// Settlement value in cents ($1.00 winner / $0.00 loser for binaries).
    #[serde(default)]
    pub settlement_value: Option<i64>,
    #[serde(default)]
    pub additional_metadata: Option<serde_json::Value>,
}

impl LifecycleMsg {
    /// Resolve the outcome label, preferring the top-level `result` and
    /// falling back to `additional_metadata.result`. Returns `None` when the
    /// outcome is genuinely absent (undetermined edge case).
    pub fn result(&self) -> Option<String> {
        if let Some(r) = &self.result {
            if !r.is_empty() {
                return Some(r.clone());
            }
        }
        self.additional_metadata
            .as_ref()
            .and_then(|m| m.get("result"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    }
}

/// We trigger the GCS write on `determined` (result known, seconds after
/// close). `settled` follows but is a no-op once the record exists.
pub fn is_settlement_trigger(event_type: &str) -> bool {
    matches!(event_type, "determined" | "settled")
}

/// Parse the outer envelope. Returns `None` on malformed JSON or a non
/// `market_lifecycle_v2` message (count + skip at the call site, never crash
/// on a single malformed event).
pub fn parse(payload: &[u8]) -> Option<LifecycleMsg> {
    let raw: RawLifecycleMessage = serde_json::from_slice(payload).ok()?;
    if raw.msg_type != "market_lifecycle_v2" {
        return None;
    }
    if raw.msg.market_ticker.is_empty() || raw.msg.event_type.is_empty() {
        return None;
    }
    Some(raw.msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Realistic `determined` event for a 15-minute crypto market. Shape copied
    // from the ssmd-cache lifecycle fixtures, plus settlement_value.
    const DETERMINED_JSON: &str = r#"{
        "type": "market_lifecycle_v2",
        "sid": 42,
        "msg": {
            "market_ticker": "KXBTC15M-26JUN031400-15",
            "event_ticker": "KXBTC15M-26JUN031400",
            "event_type": "determined",
            "open_ts": 1717423200,
            "close_ts": 1717424100,
            "determination_ts": 1717424105,
            "settled_ts": 1717424160,
            "result": "yes",
            "settlement_value": 100,
            "additional_metadata": {"payout": 1.0}
        }
    }"#;

    #[test]
    fn parses_determined_event_fields() {
        let msg = parse(DETERMINED_JSON.as_bytes()).expect("should parse");
        assert_eq!(msg.event_type, "determined");
        assert_eq!(msg.market_ticker, "KXBTC15M-26JUN031400-15");
        assert_eq!(msg.event_ticker.as_deref(), Some("KXBTC15M-26JUN031400"));
        assert_eq!(msg.result(), Some("yes".to_string()));
        assert_eq!(msg.determination_ts, Some(1717424105));
        assert_eq!(msg.close_ts, Some(1717424100));
        assert_eq!(msg.settlement_value, Some(100));
    }

    #[test]
    fn result_falls_back_to_additional_metadata() {
        let json = r#"{
            "type": "market_lifecycle_v2",
            "msg": {
                "market_ticker": "KXBTC15M-26JUN031400-15",
                "event_type": "determined",
                "additional_metadata": {"result": "no"}
            }
        }"#;
        let msg = parse(json.as_bytes()).expect("should parse");
        assert!(msg.result.is_none());
        assert_eq!(msg.result(), Some("no".to_string()));
    }

    #[test]
    fn empty_top_level_result_falls_back() {
        let json = r#"{
            "type": "market_lifecycle_v2",
            "msg": {
                "market_ticker": "KXBTC15M-26JUN031400-15",
                "event_type": "determined",
                "result": "",
                "additional_metadata": {"result": "void"}
            }
        }"#;
        let msg = parse(json.as_bytes()).expect("should parse");
        assert_eq!(msg.result(), Some("void".to_string()));
    }

    #[test]
    fn missing_result_everywhere_is_none() {
        let json = r#"{
            "type": "market_lifecycle_v2",
            "msg": {
                "market_ticker": "KXBTC15M-26JUN031400-15",
                "event_type": "determined"
            }
        }"#;
        let msg = parse(json.as_bytes()).expect("should parse");
        assert_eq!(msg.result(), None);
    }

    #[test]
    fn is_settlement_trigger_matches_determined_and_settled() {
        assert!(is_settlement_trigger("determined"));
        assert!(is_settlement_trigger("settled"));
        assert!(!is_settlement_trigger("activated"));
        assert!(!is_settlement_trigger("created"));
        assert!(!is_settlement_trigger("closed"));
    }

    #[test]
    fn parse_rejects_non_lifecycle_envelope() {
        let json = r#"{"type":"ticker","msg":{"market_ticker":"X","event_type":"y"}}"#;
        assert!(parse(json.as_bytes()).is_none());
    }

    #[test]
    fn parse_rejects_malformed_json() {
        assert!(parse(b"not json").is_none());
    }

    #[test]
    fn parse_rejects_empty_identifiers() {
        let json = r#"{"type":"market_lifecycle_v2","msg":{"market_ticker":"","event_type":"determined"}}"#;
        assert!(parse(json.as_bytes()).is_none());
    }
}
