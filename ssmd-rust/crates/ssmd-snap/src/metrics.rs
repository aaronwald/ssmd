use prometheus::{IntCounterVec, Opts, Registry};

pub struct Metrics {
    pub registry: Registry,
    pub messages_received: IntCounterVec,
    pub redis_writes: IntCounterVec,
    pub errors: IntCounterVec,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let messages_received = IntCounterVec::new(
            Opts::new("snap_messages_received_total", "Ticker messages received from NATS"),
            &["feed"],
        )
        .unwrap();

        let redis_writes = IntCounterVec::new(
            Opts::new("snap_redis_writes_total", "Successful Redis SET operations"),
            &["feed"],
        )
        .unwrap();

        let errors = IntCounterVec::new(
            Opts::new("snap_errors_total", "Errors encountered"),
            &["feed", "error_type"],
        )
        .unwrap();

        registry.register(Box::new(messages_received.clone())).unwrap();
        registry.register(Box::new(redis_writes.clone())).unwrap();
        registry.register(Box::new(errors.clone())).unwrap();

        Self {
            registry,
            messages_received,
            redis_writes,
            errors,
        }
    }
}
