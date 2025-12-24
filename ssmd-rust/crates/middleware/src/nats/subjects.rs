/// Helper for NATS subject formatting with environment prefix
pub struct SubjectBuilder {
    env: String,
    feed: String,
}

impl SubjectBuilder {
    pub fn new(env: impl Into<String>, feed: impl Into<String>) -> Self {
        Self {
            env: env.into(),
            feed: feed.into(),
        }
    }

    /// Build subject for trade messages: {env}.{feed}.trade.{ticker}
    pub fn trade(&self, ticker: &str) -> String {
        format!("{}.{}.trade.{}", self.env, self.feed, ticker)
    }

    /// Build subject for orderbook messages: {env}.{feed}.orderbook.{ticker}
    pub fn orderbook(&self, ticker: &str) -> String {
        format!("{}.{}.orderbook.{}", self.env, self.feed, ticker)
    }

    /// Build wildcard subject for all feed data: {env}.{feed}.>
    pub fn all(&self) -> String {
        format!("{}.{}.>", self.env, self.feed)
    }

    /// Build stream name: {ENV}_{FEED} (uppercase)
    pub fn stream_name(&self) -> String {
        format!("{}_{}", self.env.to_uppercase(), self.feed.to_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trade_subject() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.trade("BTCUSD"), "kalshi-dev.kalshi.trade.BTCUSD");
    }

    #[test]
    fn test_orderbook_subject() {
        let builder = SubjectBuilder::new("kalshi-dev", "kalshi");
        assert_eq!(builder.orderbook("BTCUSD"), "kalshi-dev.kalshi.orderbook.BTCUSD");
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
