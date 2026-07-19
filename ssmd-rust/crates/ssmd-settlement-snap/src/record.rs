//! The labeled training record (one immutable JSON object per settled market)
//! and its builder. Schema matches design spec §4. Prices stay in native
//! Kalshi cents; the model layer converts.

use serde::Serialize;

use crate::lifecycle::LifecycleMsg;
use crate::symbology::{coin_of, series_of};
use crate::ticker::LastTick;

/// Current record schema version. Bump on any breaking schema change.
pub const SCHEMA_VERSION: i64 = 1;

/// Provenance / quality of the final snap fields. Drives feature-quality
/// filtering in the model layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapSource {
    /// In-process last-tick map (best fidelity, race-free).
    Memory,
    /// Redis snap fallback (racy, may be stale).
    Redis,
    /// Secmaster `markets` row (reconciliation backstop, lowest fidelity).
    Secmaster,
    /// No final tick available anywhere; snap fields are null.
    Missing,
}

/// Lifecycle-derived trigger fields, decoupled from NATS so `build` is pure
/// and unit-testable.
#[derive(Debug, Clone)]
pub struct SettlementTrigger {
    pub market_ticker: String,
    pub event_ticker: Option<String>,
    /// Outcome label: `yes` / `no` / `void` / `None` (undetermined).
    pub result: Option<String>,
    pub settlement_value: Option<i64>,
    pub close_ts: Option<i64>,
    pub determination_ts: Option<i64>,
    pub nats_lifecycle_seq: i64,
}

impl SettlementTrigger {
    /// Build a trigger from a parsed lifecycle message and its NATS sequence.
    pub fn from_lifecycle(msg: &LifecycleMsg, nats_lifecycle_seq: i64) -> Self {
        Self {
            market_ticker: msg.market_ticker.clone(),
            event_ticker: msg.event_ticker.clone(),
            result: msg.result(),
            settlement_value: msg.settlement_value,
            close_ts: msg.close_ts,
            determination_ts: msg.determination_ts,
            nats_lifecycle_seq,
        }
    }
}

/// The labeled training row. Field order/names match spec §4 exactly.
#[derive(Debug, Clone, Serialize)]
pub struct SettlementRecord {
    pub market_ticker: String,
    pub series_ticker: String,
    pub event_ticker: Option<String>,
    pub coin: String,
    /// Label: `yes` / `no` / `void` / `null` (undetermined).
    pub result: Option<String>,
    pub settlement_value: Option<i64>,
    pub close_ts: Option<i64>,
    pub determination_ts: Option<i64>,
    pub final_yes_bid: Option<i64>,
    pub final_yes_ask: Option<i64>,
    pub final_no_bid: Option<i64>,
    pub final_no_ask: Option<i64>,
    pub final_last: Option<i64>,
    pub final_volume: Option<i64>,
    pub final_open_interest: Option<i64>,
    pub final_ticker_ts: Option<i64>,
    /// `determination_ts*1000 − final_ticker_ts*1000` (feature staleness).
    /// NOTE: `final_ticker_ts` is the timestamp of the newest ticker OBSERVATION.
    /// Because `LastTickMap::merge_update` preserves a prior non-null price field
    /// when a later partial tick omits it, an individual price field can be older
    /// than `snap_age_ms` implies — treat this as the age of the most-recently
    /// updated field (a lower bound on any single field's staleness), not a
    /// guarantee that every field is this fresh.
    pub snap_age_ms: Option<i64>,
    pub snap_source: SnapSource,
    pub nats_lifecycle_seq: i64,
    /// Epoch millis at record build (GCS write) time.
    pub captured_at: i64,
    pub schema_version: i64,
}

impl SettlementRecord {
    /// Assemble a record from the settlement trigger and (optionally) the final
    /// tick. Never drops a record: a missing tick still produces a valid
    /// labeled row with `snap_source = Missing` and null snap fields.
    ///
    /// `now_ms` is the wall-clock build time (epoch millis), injected so the
    /// builder stays pure and deterministically testable.
    pub fn build(trigger: &SettlementTrigger, last_tick: Option<LastTick>, now_ms: i64) -> Self {
        let series_ticker = series_of(&trigger.market_ticker).to_string();
        let coin = coin_of(&series_ticker);

        let snap_source = if last_tick.is_some() {
            SnapSource::Memory
        } else {
            SnapSource::Missing
        };

        let final_ticker_ts = last_tick.as_ref().map(|t| t.ts);
        let snap_age_ms = match (trigger.determination_ts, final_ticker_ts) {
            (Some(det), Some(tick_ts)) => Some((det - tick_ts) * 1000),
            _ => None,
        };

        SettlementRecord {
            market_ticker: trigger.market_ticker.clone(),
            series_ticker,
            event_ticker: trigger.event_ticker.clone(),
            coin,
            result: trigger.result.clone(),
            settlement_value: trigger.settlement_value,
            close_ts: trigger.close_ts,
            determination_ts: trigger.determination_ts,
            final_yes_bid: last_tick.as_ref().and_then(|t| t.yes_bid),
            final_yes_ask: last_tick.as_ref().and_then(|t| t.yes_ask),
            final_no_bid: last_tick.as_ref().and_then(|t| t.no_bid),
            final_no_ask: last_tick.as_ref().and_then(|t| t.no_ask),
            final_last: last_tick.as_ref().and_then(|t| t.last_price),
            final_volume: last_tick.as_ref().and_then(|t| t.volume),
            final_open_interest: last_tick.as_ref().and_then(|t| t.open_interest),
            final_ticker_ts,
            snap_age_ms,
            snap_source,
            nats_lifecycle_seq: trigger.nats_lifecycle_seq,
            captured_at: now_ms,
            schema_version: SCHEMA_VERSION,
        }
    }

    /// Build with an explicit snap source (used by the Redis/secmaster
    /// fallbacks and the reconciler, which supply a tick from a lower-fidelity
    /// source).
    pub fn build_with_source(
        trigger: &SettlementTrigger,
        last_tick: Option<LastTick>,
        snap_source: SnapSource,
        now_ms: i64,
    ) -> Self {
        let mut rec = Self::build(trigger, last_tick, now_ms);
        rec.snap_source = snap_source;
        rec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticker::LastTick;

    fn trigger(result: Option<&str>) -> SettlementTrigger {
        SettlementTrigger {
            market_ticker: "KXBTC15M-26JUN031400-15".to_string(),
            event_ticker: Some("KXBTC15M-26JUN031400".to_string()),
            result: result.map(|s| s.to_string()),
            settlement_value: Some(100),
            close_ts: Some(1717424100),
            determination_ts: Some(1717424105),
            nats_lifecycle_seq: 7,
        }
    }

    fn tick(last: i64, volume: i64, ts: i64) -> LastTick {
        LastTick {
            yes_bid: Some(96),
            yes_ask: Some(98),
            no_bid: Some(2),
            no_ask: Some(4),
            last_price: Some(last),
            volume: Some(volume),
            open_interest: Some(500),
            ts,
        }
    }

    #[test]
    fn yes_result_with_tick_populates_all_fields() {
        let rec = SettlementRecord::build(
            &trigger(Some("yes")),
            Some(tick(97, 1000, 1717424100)),
            1717424106000,
        );
        assert_eq!(rec.market_ticker, "KXBTC15M-26JUN031400-15");
        assert_eq!(rec.series_ticker, "KXBTC15M");
        assert_eq!(rec.coin, "BTC");
        assert_eq!(rec.result.as_deref(), Some("yes"));
        assert_eq!(rec.snap_source, SnapSource::Memory);
        assert_eq!(rec.final_last, Some(97));
        assert_eq!(rec.final_yes_bid, Some(96));
        assert_eq!(rec.final_ticker_ts, Some(1717424100));
        // (1717424105 - 1717424100) * 1000 = 5000
        assert_eq!(rec.snap_age_ms, Some(5000));
        assert_eq!(rec.captured_at, 1717424106000);
        assert_eq!(rec.schema_version, 1);
    }

    #[test]
    fn missing_tick_builds_with_null_snap_fields() {
        let rec = SettlementRecord::build(&trigger(Some("no")), None, 1717424106000);
        assert_eq!(rec.snap_source, SnapSource::Missing);
        assert_eq!(rec.result.as_deref(), Some("no"));
        assert!(rec.final_last.is_none());
        assert!(rec.final_yes_bid.is_none());
        assert!(rec.final_ticker_ts.is_none());
        assert!(rec.snap_age_ms.is_none());
    }

    #[test]
    fn void_result_is_preserved_not_coerced() {
        let rec = SettlementRecord::build(
            &trigger(Some("void")),
            Some(tick(0, 0, 1717424100)),
            1717424106000,
        );
        assert_eq!(rec.result.as_deref(), Some("void"));
    }

    #[test]
    fn zero_volume_tick_keeps_zero_fields() {
        let rec = SettlementRecord::build(
            &trigger(Some("no")),
            Some(tick(0, 0, 1717424100)),
            1717424106000,
        );
        assert_eq!(rec.final_last, Some(0));
        assert_eq!(rec.final_volume, Some(0));
        assert_eq!(rec.snap_source, SnapSource::Memory);
    }

    #[test]
    fn json_has_all_schema_keys() {
        let rec = SettlementRecord::build(
            &trigger(Some("yes")),
            Some(tick(97, 1000, 1717424100)),
            1717424106000,
        );
        let v: serde_json::Value = serde_json::to_value(&rec).unwrap();
        let obj = v.as_object().unwrap();
        for key in [
            "market_ticker",
            "series_ticker",
            "event_ticker",
            "coin",
            "result",
            "settlement_value",
            "close_ts",
            "determination_ts",
            "final_yes_bid",
            "final_yes_ask",
            "final_no_bid",
            "final_no_ask",
            "final_last",
            "final_volume",
            "final_open_interest",
            "final_ticker_ts",
            "snap_age_ms",
            "snap_source",
            "nats_lifecycle_seq",
            "captured_at",
            "schema_version",
        ] {
            assert!(obj.contains_key(key), "missing key {key}");
        }
        assert_eq!(obj["snap_source"], "memory");
    }

    #[test]
    fn null_result_serializes_as_json_null() {
        let rec = SettlementRecord::build(&trigger(None), None, 1717424106000);
        let v: serde_json::Value = serde_json::to_value(&rec).unwrap();
        assert!(v["result"].is_null());
    }

    #[test]
    fn build_with_source_overrides_snap_source() {
        let rec = SettlementRecord::build_with_source(
            &trigger(Some("yes")),
            Some(tick(97, 1000, 1717424100)),
            SnapSource::Secmaster,
            1717424106000,
        );
        assert_eq!(rec.snap_source, SnapSource::Secmaster);
    }
}
