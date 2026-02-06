use std::sync::Arc;

use dashmap::DashMap;
use tracing::warn;

/// Sanitize an input string for safe use as a NATS subject token.
///
/// NATS subjects use `.` as a level separator and `*`/`>` as wildcards.
/// Spaces are also invalid in subject tokens. This function:
/// - Replaces `/` with `-` (common in exchange symbols like "BTC/USD")
/// - Strips any character not in the allowlist: alphanumeric, `-`, `_`
/// - Truncates to `max_len` characters (default 128)
/// - Logs a warning if the input was modified
///
/// # Examples
/// ```
/// use ssmd_middleware::nats::subjects::sanitize_subject_token;
/// assert_eq!(sanitize_subject_token("BTC/USD"), "BTC-USD");
/// assert_eq!(sanitize_subject_token("safe-token_123"), "safe-token_123");
/// assert_eq!(sanitize_subject_token("inject.*.>"), "inject");
/// ```
pub fn sanitize_subject_token(input: &str) -> String {
    sanitize_subject_token_with_max_len(input, 128)
}

/// Sanitize with a custom max length (for testing or special cases).
pub fn sanitize_subject_token_with_max_len(input: &str, max_len: usize) -> String {
    let sanitized: String = input
        .replace('/', "-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(max_len)
        .collect();

    if sanitized != input && !input.is_empty() {
        warn!(
            original = %input,
            sanitized = %sanitized,
            "NATS subject token was sanitized"
        );
    }

    sanitized
}

/// Helper for NATS subject formatting with environment prefix.
/// Caches formatted subjects to avoid repeated allocations in hot path.
pub struct SubjectBuilder {
    /// Pre-computed base prefix: "{env}.{feed}."
    base_prefix: Arc<str>,
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
    /// Create a new SubjectBuilder with default prefix: {env}.{feed}
    pub fn new(env: impl Into<String>, feed: impl Into<String>) -> Self {
        let env = env.into();
        let feed = feed.into();
        let prefix = format!("{}.{}", env, feed);
        let stream_name = format!("{}_{}", env.to_uppercase(), feed.to_uppercase());
        Self::with_prefix(prefix, stream_name)
    }

    /// Create a new SubjectBuilder with a custom prefix and stream name.
    /// Use this for sharding connectors to different NATS streams.
    ///
    /// Example:
    /// ```ignore
    /// let builder = SubjectBuilder::with_prefix("prod.kalshi.main", "PROD_KALSHI");
    /// assert_eq!(builder.json_trade("KXTEST"), "prod.kalshi.main.json.trade.KXTEST");
    /// ```
    pub fn with_prefix(prefix: impl Into<String>, stream_name: impl Into<String>) -> Self {
        let prefix = prefix.into();
        let stream_name = stream_name.into();

        // Pre-compute static subjects at construction time
        let base_prefix: Arc<str> = format!("{}.", prefix).into();
        let trade_prefix: Arc<str> = format!("{}.trade.", prefix).into();
        let ticker_prefix: Arc<str> = format!("{}.ticker.", prefix).into();
        let wildcard: Arc<str> = format!("{}.>", prefix).into();
        let stream_name: Arc<str> = stream_name.into();

        Self {
            base_prefix,
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

    /// Build subject for JSON trade messages: {env}.{feed}.json.trade.{ticker}
    /// Not cached - allocates each call (acceptable for MVP volume).
    pub fn json_trade(&self, ticker: &str) -> String {
        format!("{}json.trade.{}", self.base_prefix, ticker)
    }

    /// Build subject for JSON ticker messages: {env}.{feed}.json.ticker.{ticker}
    pub fn json_ticker(&self, ticker: &str) -> String {
        format!("{}json.ticker.{}", self.base_prefix, ticker)
    }

    /// Build subject for JSON orderbook messages: {env}.{feed}.json.orderbook.{ticker}
    pub fn json_orderbook(&self, ticker: &str) -> String {
        format!("{}json.orderbook.{}", self.base_prefix, ticker)
    }

    /// Build subject for JSON lifecycle messages: {env}.{feed}.json.lifecycle.{ticker}
    pub fn json_lifecycle(&self, ticker: &str) -> String {
        format!("{}json.lifecycle.{}", self.base_prefix, ticker)
    }

    /// Build subject for JSON event lifecycle messages: {env}.{feed}.json.event_lifecycle.{ticker}
    pub fn json_event_lifecycle(&self, ticker: &str) -> String {
        format!("{}json.event_lifecycle.{}", self.base_prefix, ticker)
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

    #[test]
    fn test_json_trade_subject() {
        let builder = SubjectBuilder::new("prod", "kalshi");
        assert_eq!(builder.json_trade("INXD-25001"), "prod.kalshi.json.trade.INXD-25001");
    }

    #[test]
    fn test_json_ticker_subject() {
        let builder = SubjectBuilder::new("prod", "kalshi");
        assert_eq!(builder.json_ticker("KXBTC-25001"), "prod.kalshi.json.ticker.KXBTC-25001");
    }

    #[test]
    fn test_json_orderbook_subject() {
        let builder = SubjectBuilder::new("prod", "kalshi");
        assert_eq!(builder.json_orderbook("INXD-25001"), "prod.kalshi.json.orderbook.INXD-25001");
    }

    #[test]
    fn test_with_prefix() {
        let builder = SubjectBuilder::with_prefix("prod.kalshi.main", "PROD_KALSHI");
        assert_eq!(builder.json_trade("KXTEST-123"), "prod.kalshi.main.json.trade.KXTEST-123");
        assert_eq!(builder.json_ticker("KXTEST-123"), "prod.kalshi.main.json.ticker.KXTEST-123");
        assert_eq!(builder.all(), "prod.kalshi.main.>");
        assert_eq!(builder.stream_name(), "PROD_KALSHI");
    }

    #[test]
    fn test_with_prefix_politics() {
        let builder = SubjectBuilder::with_prefix("prod.kalshi.politics", "PROD_KALSHI_POLITICS");
        assert_eq!(builder.json_trade("KXTRUMP-25"), "prod.kalshi.politics.json.trade.KXTRUMP-25");
        assert_eq!(builder.stream_name(), "PROD_KALSHI_POLITICS");
    }

    #[test]
    fn test_json_lifecycle_subject() {
        let builder = SubjectBuilder::new("prod", "kalshi");
        assert_eq!(
            builder.json_lifecycle("KXBTCD-26JAN2310-T105000"),
            "prod.kalshi.json.lifecycle.KXBTCD-26JAN2310-T105000"
        );
    }

    #[test]
    fn test_json_event_lifecycle_subject() {
        let builder = SubjectBuilder::new("prod", "kalshi");
        assert_eq!(
            builder.json_event_lifecycle("KXBTCD-26JAN2310"),
            "prod.kalshi.json.event_lifecycle.KXBTCD-26JAN2310"
        );
    }

    // --- sanitize_subject_token tests ---

    #[test]
    fn test_sanitize_passthrough() {
        assert_eq!(sanitize_subject_token("safe-token_123"), "safe-token_123");
        assert_eq!(sanitize_subject_token("KXBTC-25001"), "KXBTC-25001");
    }

    #[test]
    fn test_sanitize_slash_replacement() {
        assert_eq!(sanitize_subject_token("BTC/USD"), "BTC-USD");
        assert_eq!(sanitize_subject_token("ETH/USD"), "ETH-USD");
    }

    #[test]
    fn test_sanitize_strips_nats_specials() {
        assert_eq!(sanitize_subject_token("inject.*.>"), "inject");
        assert_eq!(sanitize_subject_token("foo.bar"), "foobar");
        assert_eq!(sanitize_subject_token("test > wildcard"), "testwildcard");
        assert_eq!(sanitize_subject_token("a*b"), "ab");
    }

    #[test]
    fn test_sanitize_strips_spaces() {
        assert_eq!(sanitize_subject_token("hello world"), "helloworld");
    }

    #[test]
    fn test_sanitize_empty() {
        assert_eq!(sanitize_subject_token(""), "");
    }

    #[test]
    fn test_sanitize_polymarket_condition_id() {
        // Polymarket condition IDs are hex strings like 0x1234abcd...
        assert_eq!(
            sanitize_subject_token("0x1234abcdef5678"),
            "0x1234abcdef5678"
        );
    }

    #[test]
    fn test_sanitize_long_token_id() {
        // Polymarket token IDs are very long numeric strings
        let long_id = "7".repeat(200);
        let result = sanitize_subject_token(&long_id);
        assert_eq!(result.len(), 128);
        assert!(result.chars().all(|c| c == '7'));
    }

    #[test]
    fn test_sanitize_with_custom_max_len() {
        let result = sanitize_subject_token_with_max_len("abcdefghij", 5);
        assert_eq!(result, "abcde");
    }
}
