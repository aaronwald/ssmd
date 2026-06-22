//! Prometheus metrics for ssmd-bar-cache.
//!
//! Exposed at `/metrics` on the configured listen address (mirrors ssmd-snap).

use prometheus::{IntCounterVec, IntGaugeVec, Opts, Registry};

/// Metric handles shared across consumer tasks and the HTTP server.
pub struct Metrics {
    pub registry: Registry,
    /// 1s aggregates / trades received from NATS, by feed.
    pub messages_received: IntCounterVec,
    /// Bars written to the Redis ring, by feed.
    pub bars_written: IntCounterVec,
    /// Freshness: epoch seconds of the most recent bar's `end_ts_ms`, by
    /// feed+symbol. Lets alerting detect a stalled feed/symbol.
    pub last_bar_ts: IntGaugeVec,
    /// Errors encountered, by feed and error type.
    pub errors: IntCounterVec,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let messages_received = IntCounterVec::new(
            Opts::new(
                "ssmd_ohlcv_bar_cache_messages_received_total",
                "1s aggregates / trades received from NATS",
            ),
            &["feed"],
        )
        .expect("valid messages_received metric");

        let bars_written = IntCounterVec::new(
            Opts::new(
                "ssmd_ohlcv_bar_cache_bars_written_total",
                "1-minute bars written to the Redis ring",
            ),
            &["feed"],
        )
        .expect("valid bars_written metric");

        let last_bar_ts = IntGaugeVec::new(
            Opts::new(
                "ssmd_ohlcv_bar_cache_last_bar_ts",
                "Epoch seconds of the most recent bar's end timestamp",
            ),
            &["feed", "sym"],
        )
        .expect("valid last_bar_ts metric");

        let errors = IntCounterVec::new(
            Opts::new(
                "ssmd_ohlcv_bar_cache_errors_total",
                "Errors encountered while consuming or writing",
            ),
            &["feed", "error_type"],
        )
        .expect("valid errors metric");

        registry
            .register(Box::new(messages_received.clone()))
            .expect("register messages_received");
        registry
            .register(Box::new(bars_written.clone()))
            .expect("register bars_written");
        registry
            .register(Box::new(last_bar_ts.clone()))
            .expect("register last_bar_ts");
        registry
            .register(Box::new(errors.clone()))
            .expect("register errors");

        Self {
            registry,
            messages_received,
            bars_written,
            last_bar_ts,
            errors,
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
