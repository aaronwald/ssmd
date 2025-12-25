use std::sync::Arc;

use dashmap::DashMap;

/// Helper for NATS subject formatting with environment prefix.
/// Caches formatted subjects to avoid repeated allocations in hot path.
pub struct SubjectBuilder {
    /// Pre-computed prefix: "{env}.{feed}.trade."
    trade_prefix: Arc<str>,
    /// Pre-computed prefix: "{env}.{feed}.ticker."
    ticker_prefix: Arc<str>,
    /// Pre-computed wildcard subject
    wildcard: Arc<str>,
    /// Pre-computed stream name (uppercase)
    stream_name: Arc<str>,
    /// Cache of ticker -> full trade subject
    trade_cache: DashMap<Arc<str>, Arc<str>>,
    /// Cache of ticker -> full ticker subject
    ticker_cache: DashMap<Arc<str>, Arc<str>>,
}

impl SubjectBuilder {
    pub fn new(env: impl Into<String>, feed: impl Into<String>) -> Self {
        let env = env.into();
        let feed = feed.into();

        // Pre-compute static subjects at construction time
        let trade_prefix: Arc<str> = format!("{}.{}.trade.", env, feed).into();
        let ticker_prefix: Arc<str> = format!("{}.{}.ticker.", env, feed).into();
        let wildcard: Arc<str> = format!("{}.{}.>", env, feed).into();
        let stream_name: Arc<str> = format!("{}_{}", env.to_uppercase(), feed.to_uppercase()).into();

        Self {
            trade_prefix,
            ticker_prefix,
            wildcard,
            stream_name,
            trade_cache: DashMap::new(),
            ticker_cache: DashMap::new(),
        }
    }

    /// Build subject for trade messages: {env}.{feed}.trade.{ticker}
    /// Cached - first call allocates, subsequent calls return Arc clone (cheap).
    #[inline]
    pub fn trade(&self, ticker: &str) -> Arc<str> {
        // Fast path: check cache first
        if let Some(cached) = self.trade_cache.get(ticker) {
            return Arc::clone(cached.value());
        }

        // Slow path: format and cache
        let ticker_arc: Arc<str> = ticker.into();
        let subject: Arc<str> = format!("{}{}", self.trade_prefix, ticker).into();
        self.trade_cache.insert(Arc::clone(&ticker_arc), Arc::clone(&subject));
        subject
    }

    /// Build subject for ticker messages: {env}.{feed}.ticker.{ticker}
    /// Cached - first call allocates, subsequent calls return Arc clone (cheap).
    #[inline]
    pub fn ticker(&self, ticker: &str) -> Arc<str> {
        // Fast path: check cache first
        if let Some(cached) = self.ticker_cache.get(ticker) {
            return Arc::clone(cached.value());
        }

        // Slow path: format and cache
        let ticker_arc: Arc<str> = ticker.into();
        let subject: Arc<str> = format!("{}{}", self.ticker_prefix, ticker).into();
        self.ticker_cache.insert(Arc::clone(&ticker_arc), Arc::clone(&subject));
        subject
    }

    /// Build wildcard subject for all feed data: {env}.{feed}.>
    /// Pre-computed at construction time.
    #[inline]
    pub fn all(&self) -> &str {
        &self.wildcard
    }

    /// Build stream name: {ENV}_{FEED} (uppercase)
    /// Pre-computed at construction time.
    #[inline]
    pub fn stream_name(&self) -> &str {
        &self.stream_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trade_subject() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.trade("BTCUSD").as_ref(), "kalshi-dev.kalshi.trade.BTCUSD");
    }

    #[test]
    fn test_trade_subject_cached() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        let first = builder.trade("BTCUSD");
        let second = builder.trade("BTCUSD");
        // Should return same Arc (pointer equality)
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn test_ticker_subject() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.ticker("KXTEST-123").as_ref(), "kalshi-dev.kalshi.ticker.KXTEST-123");
    }

    #[test]
    fn test_ticker_subject_cached() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        let first = builder.ticker("KXTEST-123");
        let second = builder.ticker("KXTEST-123");
        // Should return same Arc (pointer equality)
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn test_wildcard_subject() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.all(), "kalshi-dev.kalshi.>");
    }

    #[test]
    fn test_stream_name() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.stream_name(), "KALSHI-DEV_KALSHI");
    }
}
