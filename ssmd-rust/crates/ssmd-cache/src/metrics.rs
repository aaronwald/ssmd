use prometheus::{IntCounterVec, IntCounter, Gauge, Opts, Registry};

#[derive(Clone)]
pub struct CacheMetrics {
    pub cdc_events: IntCounterVec,
    pub last_event_timestamp: Gauge,
    pub gaps: IntCounter,
    pub skipped: IntCounter,
    pub redis_writes: IntCounterVec,
}

impl CacheMetrics {
    pub fn new(registry: &Registry) -> prometheus::Result<Self> {
        let cdc_events = IntCounterVec::new(
            Opts::new("ssmd_cache_cdc_events_total", "CDC events processed"),
            &["table", "operation"],
        )?;
        let last_event_timestamp = Gauge::with_opts(
            Opts::new("ssmd_cache_cdc_last_event_timestamp", "Unix epoch of last CDC event"),
        )?;
        let gaps = IntCounter::with_opts(
            Opts::new("ssmd_cache_cdc_gaps_total", "LSN gaps detected"),
        )?;
        let skipped = IntCounter::with_opts(
            Opts::new("ssmd_cache_cdc_skipped_total", "Events skipped (LSN before snapshot)"),
        )?;
        let redis_writes = IntCounterVec::new(
            Opts::new("ssmd_cache_redis_writes_total", "Redis HSET/HDEL operations"),
            &["operation"],
        )?;

        registry.register(Box::new(cdc_events.clone()))?;
        registry.register(Box::new(last_event_timestamp.clone()))?;
        registry.register(Box::new(gaps.clone()))?;
        registry.register(Box::new(skipped.clone()))?;
        registry.register(Box::new(redis_writes.clone()))?;

        Ok(Self {
            cdc_events,
            last_event_timestamp,
            gaps,
            skipped,
            redis_writes,
        })
    }
}
