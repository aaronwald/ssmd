//! Prometheus metrics for ssmd-settlement-snap.
//!
//! Two counters, exported on `GET /metrics` (default global registry, encoded
//! via [`encode_metrics`]):
//!
//! - `ssmd_settlement_records_written_total{coin,outcome}` — settlement records
//!   written to GCS, labelled by coin and write outcome (`written` / `exists`).
//! - `ssmd_settlement_lookup_total{source}` — final-snap source resolution
//!   counts (`memory` / `redis` / `secmaster` / `missing`).
//!
//! Counters are pre-initialized so GMP discovers the metric/series names even
//! during quiet periods (a settlement-snap that has written nothing yet still
//! exports a zero series — the alert can evaluate the absence of increase).

use once_cell::sync::Lazy;
use prometheus::{register_int_counter_vec, Encoder, IntCounterVec, TextEncoder};

const LABEL_COIN: &str = "coin";
const LABEL_OUTCOME: &str = "outcome";
const LABEL_SOURCE: &str = "source";

/// Write outcome label values.
pub const OUTCOME_WRITTEN: &str = "written";
pub const OUTCOME_EXISTS: &str = "exists";

/// Snap-source label values (mirror `record::SnapSource`).
pub const SOURCE_MEMORY: &str = "memory";
pub const SOURCE_REDIS: &str = "redis";
pub const SOURCE_SECMASTER: &str = "secmaster";
pub const SOURCE_MISSING: &str = "missing";

/// Total settlement records written to GCS, by coin and write outcome.
static RECORDS_WRITTEN_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_settlement_records_written_total",
        "Total settlement records written to GCS, labelled by coin and write outcome",
        &[LABEL_COIN, LABEL_OUTCOME]
    )
    .expect("Failed to register ssmd_settlement_records_written_total metric")
});

/// Total final-snap source resolutions, by source.
static LOOKUP_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_settlement_lookup_total",
        "Total final-snap source resolutions, labelled by source",
        &[LABEL_SOURCE]
    )
    .expect("Failed to register ssmd_settlement_lookup_total metric")
});

/// Pre-initialize the lookup-source series so every source label exists at zero
/// before the first settlement. Coins for the write counter are unbounded
/// (BTC/ETH/HYPE/…); they appear on first write — acceptable because the alert
/// sums across coins.
pub fn init_metrics() {
    for source in [
        SOURCE_MEMORY,
        SOURCE_REDIS,
        SOURCE_SECMASTER,
        SOURCE_MISSING,
    ] {
        LOOKUP_TOTAL.with_label_values(&[source]);
    }
}

/// Record a settlement record write (Written or Exists) for a coin.
pub fn inc_record_written(coin: &str, outcome: &str) {
    RECORDS_WRITTEN_TOTAL
        .with_label_values(&[coin, outcome])
        .inc();
}

/// Record a final-snap source resolution.
pub fn inc_lookup(source: &str) {
    LOOKUP_TOTAL.with_label_values(&[source]).inc();
}

/// Encode all metrics to Prometheus text format from the default global registry.
pub fn encode_metrics() -> Result<String, prometheus::Error> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    String::from_utf8(buffer)
        .map_err(|e| prometheus::Error::Msg(format!("Failed to encode metrics as UTF-8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_metrics_registers_all_lookup_sources() {
        init_metrics();
        let output = encode_metrics().expect("encode");
        assert!(output.contains("ssmd_settlement_lookup_total"));
        assert!(output.contains("source=\"memory\""));
        assert!(output.contains("source=\"missing\""));
    }

    #[test]
    fn inc_record_written_exports_series() {
        inc_record_written("BTC", OUTCOME_WRITTEN);
        inc_record_written("ETH", OUTCOME_EXISTS);
        let output = encode_metrics().expect("encode");
        assert!(output.contains("ssmd_settlement_records_written_total"));
        assert!(output.contains("coin=\"BTC\""));
        assert!(output.contains("outcome=\"written\""));
        assert!(output.contains("outcome=\"exists\""));
    }

    #[test]
    fn inc_lookup_exports_series() {
        inc_lookup(SOURCE_REDIS);
        let output = encode_metrics().expect("encode");
        assert!(output.contains("source=\"redis\""));
    }
}
