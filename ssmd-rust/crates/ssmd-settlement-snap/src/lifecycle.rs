//! Lifecycle message parsing and settlement-trigger detection.
//!
//! Mirrors the `LifecycleMsg` shape used by `ssmd-cache`, but adds
//! `settlement_value` and a `result` accessor that falls back to
//! `additional_metadata.result` when the top-level field is absent.

use serde::{Deserialize, Deserializer};

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
    ///
    /// The exchange wire form has drifted: Kalshi now sends a STRING dollar
    /// value (e.g. `"0.0000"`, `"1.0000"`), where it historically sent an
    /// integer cent count. The tolerant deserializer below accepts string,
    /// number, or null and NEVER errors — a bad/unexpected shape becomes `None`
    /// instead of failing the whole message parse (which previously dropped the
    /// entire `determined` event, losing `result` + `determination_ts`).
    #[serde(default, deserialize_with = "de_settlement_value_cents")]
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

/// Tolerantly deserialize `settlement_value` into native Kalshi cents.
///
/// The field is deserialized as a free-form `serde_json::Value` first so that
/// NO input shape can ever hard-error the enclosing `LifecycleMsg` (the exact
/// bug this fixes: a string `"0.0000"` failing a direct `i64` deserialize took
/// the whole `determined` event down with it). Conversion rules:
///
/// - null / absent -> `None`
/// - string -> parsed as a dollar value via [`crate::price::dollars_to_cents`]
///   (trims / `is_finite` / rounds / clamps to `[0, 100]`); a malformed string
///   yields `None`, never an error.
/// - integer -> accepted as-is (legacy cent wire form).
/// - non-integer number -> treated as a dollar value (rounded, clamped);
///   non-finite yields `None`.
/// - any other shape -> `None`.
fn de_settlement_value_cents<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => crate::price::dollars_to_cents(&s),
        Some(serde_json::Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                // Legacy integer wire form: already cents. Clamp to the Kalshi
                // price domain [0,100] like the string/float branches, so a
                // garbage legacy int can't persist an out-of-domain value into
                // the immutable record (matches the ticker legacy-int path).
                Some(crate::price::clamp_price_cents(i))
            } else if let Some(f) = n.as_f64() {
                // Fractional number: interpret as a dollar value.
                if f.is_finite() {
                    Some(((f * 100.0).round() as i64).clamp(0, 100))
                } else {
                    None
                }
            } else {
                None
            }
        }
        Some(_) => None,
    })
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
    fn result_resolves_from_additional_metadata_real_shape() {
        // Realistic `determined` event where the outcome lives under
        // `additional_metadata.result` (no top-level `result`), with settled_ts.
        let json = r#"{
            "type": "market_lifecycle_v2",
            "msg": {
                "market_ticker": "KXBTC15M-26JUL191145-45",
                "event_type": "determined",
                "additional_metadata": {"result": "yes"},
                "settled_ts": 1784475902
            }
        }"#;
        let msg = parse(json.as_bytes()).expect("should parse");
        assert_eq!(msg.result(), Some("yes".to_string()));
        assert_eq!(msg.settled_ts, Some(1784475902));
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

    // ---- Real captured wire payloads (ground truth from LIVE NATS) ----

    // A real `determined` event: note `settlement_value` is a STRING dollar
    // value (`"0.0000"`), not an integer. This is the shape that was silently
    // dropping the whole event (serde failing to deserialize string -> i64).
    const REAL_DETERMINED_JSON: &str = r#"{"type":"market_lifecycle_v2","sid":1,"seq":279597,"msg":{"market_ticker":"KXMLBSPREAD-26JUL191605WSHATH-ATH4","determination_ts":1784501722,"result":"no","settlement_value":"0.0000","event_type":"determined"}}"#;

    // A real `settled` event: carries only market_ticker + settled_ts +
    // event_type (no result, no determination_ts, no settlement_value).
    const REAL_SETTLED_JSON: &str = r#"{"type":"market_lifecycle_v2","sid":1,"seq":274085,"msg":{"market_ticker":"KXBNB15M-26JUL191830-30","settled_ts":1784500206,"event_type":"settled"}}"#;

    #[test]
    fn parses_real_determined_payload_with_string_settlement_value() {
        let msg =
            parse(REAL_DETERMINED_JSON.as_bytes()).expect("real determined event should parse");
        assert_eq!(msg.event_type, "determined");
        assert_eq!(msg.market_ticker, "KXMLBSPREAD-26JUL191605WSHATH-ATH4");
        assert_eq!(msg.result(), Some("no".to_string()));
        assert_eq!(msg.determination_ts, Some(1784501722));
        // String dollar value "0.0000" -> 0 cents.
        assert_eq!(msg.settlement_value, Some(0));
    }

    #[test]
    fn parses_real_settled_payload() {
        let msg = parse(REAL_SETTLED_JSON.as_bytes()).expect("real settled event should parse");
        assert_eq!(msg.event_type, "settled");
        assert_eq!(msg.market_ticker, "KXBNB15M-26JUL191830-30");
        assert_eq!(msg.settled_ts, Some(1784500206));
        assert_eq!(msg.result(), None);
        assert_eq!(msg.determination_ts, None);
        assert_eq!(msg.settlement_value, None);
    }

    // Helper: a minimal `determined` payload with a caller-supplied JSON token
    // for `settlement_value`, to exercise the tolerant deserializer directly.
    fn determined_with_settlement_value(token: &str) -> String {
        format!(
            r#"{{"type":"market_lifecycle_v2","msg":{{"market_ticker":"KXBTC15M-26JUN031400-15","event_type":"determined","settlement_value":{token}}}}}"#
        )
    }

    #[test]
    fn settlement_value_string_dollars_parse_to_cents() {
        let one = parse(determined_with_settlement_value(r#""1.0000""#).as_bytes()).expect("parse");
        assert_eq!(one.settlement_value, Some(100));
        let zero =
            parse(determined_with_settlement_value(r#""0.0000""#).as_bytes()).expect("parse");
        assert_eq!(zero.settlement_value, Some(0));
    }

    #[test]
    fn settlement_value_null_and_absent_are_none() {
        let null = parse(determined_with_settlement_value("null").as_bytes()).expect("parse");
        assert_eq!(null.settlement_value, None);
        // Absent entirely (no settlement_value key at all).
        let absent = parse(
            r#"{"type":"market_lifecycle_v2","msg":{"market_ticker":"KXBTC15M-26JUN031400-15","event_type":"determined"}}"#
                .as_bytes(),
        )
        .expect("parse");
        assert_eq!(absent.settlement_value, None);
    }

    #[test]
    fn settlement_value_malformed_string_is_none_not_error() {
        // A malformed dollar string must yield None, NEVER fail the whole struct
        // (that was the original bug — a bad field dropped the entire event).
        let msg = parse(determined_with_settlement_value(r#""not-a-price""#).as_bytes())
            .expect("malformed settlement_value must not fail the whole parse");
        assert_eq!(msg.settlement_value, None);
        assert_eq!(msg.event_type, "determined");
    }

    #[test]
    fn settlement_value_integer_legacy_form_is_accepted() {
        // Legacy integer wire form (cents) is accepted as-is when in-domain.
        let msg = parse(determined_with_settlement_value("100").as_bytes()).expect("parse");
        assert_eq!(msg.settlement_value, Some(100));
        let zero = parse(determined_with_settlement_value("0").as_bytes()).expect("parse");
        assert_eq!(zero.settlement_value, Some(0));
        // A garbage/out-of-domain legacy integer is clamped to [0,100], never
        // persisted verbatim into the immutable record.
        let big = parse(determined_with_settlement_value("999999999").as_bytes()).expect("parse");
        assert_eq!(big.settlement_value, Some(100));
        let neg = parse(determined_with_settlement_value("-5").as_bytes()).expect("parse");
        assert_eq!(neg.settlement_value, Some(0));
    }

    #[test]
    fn settlement_value_float_dollars_parse_to_cents() {
        // A bare JSON float is treated as a dollar value.
        let msg = parse(determined_with_settlement_value("0.97").as_bytes()).expect("parse");
        assert_eq!(msg.settlement_value, Some(97));
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
