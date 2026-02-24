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

    /// Comma-separated NATS stream names (e.g. PROD_KALSHI_CRYPTO,PROD_KRAKEN_FUTURES,PROD_POLYMARKET)
    #[arg(long, env = "SNAP_STREAMS")]
    pub streams: String,

    /// Redis key TTL in seconds
    #[arg(long, env = "SNAP_TTL_SECS", default_value = "300")]
    pub ttl_secs: u64,

    /// Metrics/health listen address
    #[arg(long, env = "SNAP_LISTEN_ADDR", default_value = "0.0.0.0:9090")]
    pub listen_addr: String,
}

/// Derived feed config from a stream name.
pub struct StreamConfig {
    pub stream_name: String,
    pub feed: String,
    pub filter_subject: String,
}

/// Map known stream names to feed and NATS subject filter.
///
/// Stream naming convention: PROD_{EXCHANGE}[_{CATEGORY}]
/// Feed names must match the data-ts API feed identifiers.
///
/// | Stream               | Feed             | Filter Subject                              |
/// |----------------------|------------------|---------------------------------------------|
/// | PROD_KALSHI_CRYPTO   | kalshi           | prod.kalshi.crypto.json.ticker.>            |
/// | PROD_KRAKEN_FUTURES  | kraken-futures   | prod.kraken.futures.json.ticker.>           |
/// | PROD_POLYMARKET      | polymarket       | prod.polymarket.clob.json.ticker.>          |
pub fn parse_stream(stream_name: &str) -> StreamConfig {
    let parts: Vec<&str> = stream_name.split('_').collect();
    let segments: Vec<String> = parts.iter().skip(1).map(|s| s.to_lowercase()).collect();

    // Build subject prefix from all segments after PROD
    let subject_segments = segments.join(".");
    let filter_subject = match segments.first().map(|s| s.as_str()) {
        Some("polymarket") => format!("prod.{}.clob.json.ticker.>", subject_segments),
        _ => format!("prod.{}.json.ticker.>", subject_segments),
    };

    // Map exchange name (first segment) to canonical feed name
    let feed = match segments.first().map(|s| s.as_str()) {
        Some("kalshi") => "kalshi".to_string(),
        Some("kraken") => "kraken-futures".to_string(),
        Some("polymarket") => "polymarket".to_string(),
        _ => segments.join("-"),
    };

    StreamConfig {
        stream_name: stream_name.to_string(),
        feed,
        filter_subject,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kalshi() {
        let sc = parse_stream("PROD_KALSHI_CRYPTO");
        assert_eq!(sc.feed, "kalshi");
        assert_eq!(sc.filter_subject, "prod.kalshi.crypto.json.ticker.>");
    }

    #[test]
    fn test_parse_kraken() {
        let sc = parse_stream("PROD_KRAKEN_FUTURES");
        assert_eq!(sc.feed, "kraken-futures");
        assert_eq!(sc.filter_subject, "prod.kraken.futures.json.ticker.>");
    }

    #[test]
    fn test_parse_polymarket() {
        let sc = parse_stream("PROD_POLYMARKET");
        assert_eq!(sc.feed, "polymarket");
        assert_eq!(sc.filter_subject, "prod.polymarket.clob.json.ticker.>");
    }
}
