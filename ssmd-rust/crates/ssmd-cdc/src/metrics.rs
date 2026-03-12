use once_cell::sync::Lazy;
use prometheus::{
    register_gauge, register_int_counter, register_int_counter_vec, Encoder, Gauge, IntCounter,
    IntCounterVec, TextEncoder,
};

pub static CDC_EVENTS_PUBLISHED: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_cdc_events_published_total",
        "Total CDC events published to NATS",
        &["table"]
    )
    .expect("Failed to register cdc_events_published_total metric")
});

pub static CDC_EVENTS_SKIPPED: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "ssmd_cdc_events_skipped_total",
        "Total CDC events skipped (table filter)"
    )
    .expect("Failed to register cdc_events_skipped_total metric")
});

pub static CDC_POLL_ERRORS: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "ssmd_cdc_poll_errors_total",
        "Total CDC poll failures"
    )
    .expect("Failed to register cdc_poll_errors_total metric")
});

pub static CDC_LAST_PUBLISH_TIMESTAMP: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "ssmd_cdc_last_publish_timestamp",
        "Unix epoch of last successful CDC publish"
    )
    .expect("Failed to register cdc_last_publish_timestamp metric")
});

pub static CDC_POLLS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "ssmd_cdc_polls_total",
        "Total CDC poll iterations"
    )
    .expect("Failed to register cdc_polls_total metric")
});

pub static CDC_PUBLISH_ERRORS: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        "ssmd_cdc_publish_errors_total",
        "Total CDC events that failed to publish to NATS",
        &["table"]
    )
    .expect("Failed to register cdc_publish_errors_total metric")
});

pub fn encode_metrics() -> Result<String, prometheus::Error> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    String::from_utf8(buffer).map_err(|e| {
        prometheus::Error::Msg(format!("Failed to encode metrics as UTF-8: {}", e))
    })
}
