use clap::Parser;

/// ssmd-snap: NATS ticker stream → Redis snapshot service
#[derive(Parser, Debug)]
#[command(name = "ssmd-snap")]
pub struct Config {
    /// NATS server URL
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    pub nats_url: String,

    /// Redis server URL
    #[arg(long, env = "REDIS_URL", default_value = "redis://localhost:6379")]
    pub redis_url: String,

    /// JSON array of subscriptions: [{"stream":"...","feed":"...","subject":"..."}]
    #[arg(long, env = "SNAP_SUBSCRIPTIONS")]
    pub subscriptions: String,

    /// Redis key TTL in seconds
    #[arg(long, env = "SNAP_TTL_SECS", default_value = "60")]
    pub ttl_secs: u64,

    /// Metrics/health listen address
    #[arg(long, env = "SNAP_LISTEN_ADDR", default_value = "0.0.0.0:9090")]
    pub listen_addr: String,
}

/// A single stream subscription parsed from JSON.
#[derive(Debug, serde::Deserialize)]
pub struct Subscription {
    pub stream: String,
    pub feed: String,
    pub subject: String,
}

/// Parse the subscriptions JSON string into a list of Subscription structs.
pub fn parse_subscriptions(json_str: &str) -> Vec<Subscription> {
    serde_json::from_str(json_str).expect("failed to parse SNAP_SUBSCRIPTIONS JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_subscriptions() {
        let json = r#"[
            {"stream":"PROD_KALSHI_CRYPTO","feed":"kalshi","subject":"prod.kalshi.crypto.json.ticker.>"},
            {"stream":"PROD_KRAKEN_FUTURES","feed":"kraken-futures","subject":"prod.kraken-futures.json.ticker.>"}
        ]"#;
        let subs = parse_subscriptions(json);
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].stream, "PROD_KALSHI_CRYPTO");
        assert_eq!(subs[0].feed, "kalshi");
        assert_eq!(subs[0].subject, "prod.kalshi.crypto.json.ticker.>");
        assert_eq!(subs[1].feed, "kraken-futures");
        assert_eq!(subs[1].subject, "prod.kraken-futures.json.ticker.>");
    }
}
