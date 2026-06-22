use std::env;

/// Runtime configuration for ssmd-bar-cache.
///
/// Built from environment variables via [`Config::from_env`]. `NATS_URL` and
/// `REDIS_URL` are required (no sensible default for the deployment targets);
/// everything else has a default suitable for prod.
#[derive(Debug, Clone)]
pub struct Config {
    /// NATS server URL (required).
    pub nats_url: String,
    /// Redis server URL (required).
    pub redis_url: String,

    /// JetStream subject for massive 1s OHLCV aggregates.
    pub massive_subject: String,
    /// JetStream stream name carrying the massive aggregates.
    pub massive_stream: String,

    /// JetStream subject for kraken-spot trades.
    pub kraken_subject: String,
    /// JetStream stream name carrying the kraken-spot trades.
    pub kraken_stream: String,

    /// Number of 1-minute bars retained per symbol in the Redis ring.
    pub ring: usize,
    /// TTL (seconds) applied to each Redis ring key.
    pub ttl_secs: u64,

    /// Health/metrics HTTP listen address.
    pub listen_addr: String,
}

impl Config {
    /// Build a [`Config`] from process environment variables.
    ///
    /// Panics with a clear message if `NATS_URL` or `REDIS_URL` is missing,
    /// since the service cannot do its job without either.
    pub fn from_env() -> Self {
        let nats_url = require_env("NATS_URL");
        let redis_url = require_env("REDIS_URL");

        let massive_subject = env_or("BAR_CACHE_MASSIVE_SUBJECT", "prod.massive.json.ohlcv_1s.>");
        let massive_stream = env_or("BAR_CACHE_MASSIVE_STREAM", "PROD_MASSIVE");

        let kraken_subject = env_or("BAR_CACHE_KRAKEN_SUBJECT", "prod.kraken-spot.json.trade.>");
        let kraken_stream = env_or("BAR_CACHE_KRAKEN_STREAM", "PROD_KRAKEN_SPOT");

        let ring = parse_env("BAR_CACHE_RING", 60usize);
        let ttl_secs = parse_env("BAR_CACHE_TTL_SECS", 3700u64);

        let listen_addr = env_or("BAR_CACHE_LISTEN_ADDR", "0.0.0.0:8080");

        Config {
            nats_url,
            redis_url,
            massive_subject,
            massive_stream,
            kraken_subject,
            kraken_stream,
            ring,
            ttl_secs,
            listen_addr,
        }
    }
}

/// Read a required env var or panic with a clear, actionable message.
fn require_env(key: &str) -> String {
    match env::var(key) {
        Ok(v) if !v.is_empty() => v,
        _ => panic!("{key} is required but not set (or empty)"),
    }
}

/// Read an env var, falling back to a default string when unset/empty.
fn env_or(key: &str, default: &str) -> String {
    match env::var(key) {
        Ok(v) if !v.is_empty() => v,
        _ => default.to_string(),
    }
}

/// Read and parse an env var, falling back to a default on unset/empty/invalid.
fn parse_env<T>(key: &str, default: T) -> T
where
    T: std::str::FromStr,
{
    match env::var(key) {
        Ok(v) if !v.is_empty() => v.parse().unwrap_or(default),
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global; serialize these tests so set/unset in one
    // test does not race with another. Recover from poisoning since the
    // should_panic tests intentionally panic while holding the guard.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn clear_all() {
        for key in [
            "NATS_URL",
            "REDIS_URL",
            "BAR_CACHE_MASSIVE_SUBJECT",
            "BAR_CACHE_MASSIVE_STREAM",
            "BAR_CACHE_KRAKEN_SUBJECT",
            "BAR_CACHE_KRAKEN_STREAM",
            "BAR_CACHE_RING",
            "BAR_CACHE_TTL_SECS",
            "BAR_CACHE_LISTEN_ADDR",
        ] {
            env::remove_var(key);
        }
    }

    #[test]
    fn from_env_uses_defaults_when_only_required_set() {
        let _guard = lock();
        clear_all();
        env::set_var("NATS_URL", "nats://localhost:4222");
        env::set_var("REDIS_URL", "redis://localhost:6379");

        let cfg = Config::from_env();

        assert_eq!(cfg.nats_url, "nats://localhost:4222");
        assert_eq!(cfg.redis_url, "redis://localhost:6379");
        assert_eq!(cfg.massive_subject, "prod.massive.json.ohlcv_1s.>");
        assert_eq!(cfg.massive_stream, "PROD_MASSIVE");
        assert_eq!(cfg.kraken_subject, "prod.kraken-spot.json.trade.>");
        assert_eq!(cfg.kraken_stream, "PROD_KRAKEN_SPOT");
        assert_eq!(cfg.ring, 60);
        assert_eq!(cfg.ttl_secs, 3700);
        assert_eq!(cfg.listen_addr, "0.0.0.0:8080");

        clear_all();
    }

    #[test]
    fn from_env_reads_overrides() {
        let _guard = lock();
        clear_all();
        env::set_var("NATS_URL", "nats://nats:4222");
        env::set_var("REDIS_URL", "redis://redis:6379");
        env::set_var("BAR_CACHE_MASSIVE_SUBJECT", "x.ohlcv.>");
        env::set_var("BAR_CACHE_MASSIVE_STREAM", "X_MASSIVE");
        env::set_var("BAR_CACHE_KRAKEN_SUBJECT", "x.trade.>");
        env::set_var("BAR_CACHE_KRAKEN_STREAM", "X_KRAKEN");
        env::set_var("BAR_CACHE_RING", "120");
        env::set_var("BAR_CACHE_TTL_SECS", "7400");
        env::set_var("BAR_CACHE_LISTEN_ADDR", "0.0.0.0:9999");

        let cfg = Config::from_env();

        assert_eq!(cfg.massive_subject, "x.ohlcv.>");
        assert_eq!(cfg.massive_stream, "X_MASSIVE");
        assert_eq!(cfg.kraken_subject, "x.trade.>");
        assert_eq!(cfg.kraken_stream, "X_KRAKEN");
        assert_eq!(cfg.ring, 120);
        assert_eq!(cfg.ttl_secs, 7400);
        assert_eq!(cfg.listen_addr, "0.0.0.0:9999");

        clear_all();
    }

    #[test]
    fn parse_env_falls_back_on_invalid() {
        let _guard = lock();
        clear_all();
        env::set_var("BAR_CACHE_RING", "not-a-number");
        assert_eq!(parse_env("BAR_CACHE_RING", 60usize), 60);
        clear_all();
    }

    #[test]
    #[should_panic(expected = "NATS_URL is required")]
    fn from_env_panics_without_nats_url() {
        let _guard = lock();
        clear_all();
        env::set_var("REDIS_URL", "redis://localhost:6379");
        let _ = Config::from_env();
    }

    #[test]
    #[should_panic(expected = "REDIS_URL is required")]
    fn from_env_panics_without_redis_url() {
        let _guard = lock();
        clear_all();
        env::set_var("NATS_URL", "nats://localhost:4222");
        env::remove_var("REDIS_URL");
        let _ = Config::from_env();
    }
}
