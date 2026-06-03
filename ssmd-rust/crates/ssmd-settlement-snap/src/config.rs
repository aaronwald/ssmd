//! Environment-driven configuration. Fails loud on missing required vars at
//! startup (not at first use), per the defensive-coding connection/config rule.

use anyhow::{anyhow, Result};

/// Runtime configuration for the settlement-snap service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// NATS server URL. REQUIRED — no default.
    pub nats_url: String,
    /// JetStream stream carrying lifecycle events.
    pub lifecycle_stream: String,
    /// Subject filter for the durable lifecycle (trigger) consumer.
    pub lifecycle_subject: String,
    /// Subject filter for the ephemeral LastPerSubject ticker consumer.
    pub ticker_subject: String,
    /// GCS bucket for settled records.
    pub gcs_bucket: String,
    /// Object-key prefix for the raw settled JSON.
    pub gcs_prefix: String,
    /// Series suffix identifying the markets we capture (e.g. "15M").
    pub series_suffix: String,
    /// Optional Redis URL used as a last-tick fallback when the in-process map
    /// is cold (e.g. after a restart).
    pub redis_url: Option<String>,
    /// Optional Postgres URL used by the startup reconciliation backfill.
    pub database_url: Option<String>,
    /// Durable consumer name for the lifecycle stream.
    pub consumer_name: String,
}

const DEFAULT_LIFECYCLE_STREAM: &str = "PROD_KALSHI_LIFECYCLE";
const DEFAULT_LIFECYCLE_SUBJECT: &str = "prod.kalshi.json.lifecycle.>";
const DEFAULT_TICKER_SUBJECT: &str = "prod.kalshi.crypto.json.ticker.>";
const DEFAULT_GCS_BUCKET: &str = "ssmd-data";
const DEFAULT_GCS_PREFIX: &str = "settled/kalshi/crypto";
const DEFAULT_SERIES_SUFFIX: &str = "15M";
const DEFAULT_CONSUMER_NAME: &str = "settlement-snap-v1";

impl Config {
    /// Load configuration from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_getter(|key| std::env::var(key).ok())
    }

    /// Load configuration via an injectable getter (pure / testable).
    ///
    /// `get` returns `Some(value)` for a set, non-empty-or-empty var and `None`
    /// when unset. Required vars that are unset OR empty are rejected loudly.
    pub fn from_getter(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let nats_url = required(&get, "NATS_URL")?;

        Ok(Self {
            nats_url,
            lifecycle_stream: optional(&get, "LIFECYCLE_STREAM", DEFAULT_LIFECYCLE_STREAM),
            lifecycle_subject: optional(&get, "LIFECYCLE_SUBJECT", DEFAULT_LIFECYCLE_SUBJECT),
            ticker_subject: optional(&get, "TICKER_SUBJECT", DEFAULT_TICKER_SUBJECT),
            gcs_bucket: optional(&get, "GCS_BUCKET", DEFAULT_GCS_BUCKET),
            gcs_prefix: optional(&get, "GCS_PREFIX", DEFAULT_GCS_PREFIX),
            series_suffix: optional(&get, "SERIES_SUFFIX", DEFAULT_SERIES_SUFFIX),
            redis_url: optional_opt(&get, "REDIS_URL"),
            database_url: optional_opt(&get, "DATABASE_URL"),
            consumer_name: optional(&get, "CONSUMER_NAME", DEFAULT_CONSUMER_NAME),
        })
    }
}

/// Fetch a required var, rejecting unset OR empty values with context.
fn required(get: &impl Fn(&str) -> Option<String>, key: &str) -> Result<String> {
    match get(key) {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(anyhow!("required env var {key} is unset or empty")),
    }
}

/// Fetch an optional var with a default, treating empty as unset.
fn optional(get: &impl Fn(&str) -> Option<String>, key: &str, default: &str) -> String {
    match get(key) {
        Some(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

/// Fetch an optional var that has no default, treating empty as `None`.
fn optional_opt(get: &impl Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    match get(key) {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn getter(map: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |key| map.get(key).map(|s| s.to_string())
    }

    #[test]
    fn missing_nats_url_is_fail_loud() {
        let cfg = Config::from_getter(getter(HashMap::new()));
        let err = cfg.unwrap_err().to_string();
        assert!(err.contains("NATS_URL"), "error was: {err}");
    }

    #[test]
    fn empty_nats_url_is_rejected() {
        let mut m = HashMap::new();
        m.insert("NATS_URL", "  ");
        let cfg = Config::from_getter(getter(m));
        assert!(cfg.is_err());
    }

    #[test]
    fn minimal_env_uses_defaults() {
        let mut m = HashMap::new();
        m.insert("NATS_URL", "nats://localhost:4222");
        let cfg = Config::from_getter(getter(m)).expect("should load");
        assert_eq!(cfg.nats_url, "nats://localhost:4222");
        assert_eq!(cfg.lifecycle_stream, "PROD_KALSHI_LIFECYCLE");
        assert_eq!(cfg.lifecycle_subject, "prod.kalshi.json.lifecycle.>");
        assert_eq!(cfg.ticker_subject, "prod.kalshi.crypto.json.ticker.>");
        assert_eq!(cfg.gcs_bucket, "ssmd-data");
        assert_eq!(cfg.gcs_prefix, "settled/kalshi/crypto");
        assert_eq!(cfg.series_suffix, "15M");
        assert_eq!(cfg.consumer_name, "settlement-snap-v1");
        assert!(cfg.redis_url.is_none());
        assert!(cfg.database_url.is_none());
    }

    #[test]
    fn fully_set_env_parses_into_struct() {
        let mut m = HashMap::new();
        m.insert("NATS_URL", "nats://nats.nats:4222");
        m.insert("LIFECYCLE_STREAM", "STREAM_X");
        m.insert("LIFECYCLE_SUBJECT", "x.lifecycle.>");
        m.insert("TICKER_SUBJECT", "x.ticker.>");
        m.insert("GCS_BUCKET", "my-bucket");
        m.insert("GCS_PREFIX", "settled/x");
        m.insert("SERIES_SUFFIX", "5M");
        m.insert("REDIS_URL", "redis://redis:6379");
        m.insert("DATABASE_URL", "postgres://u:p@db/ssmd");
        m.insert("CONSUMER_NAME", "snap-v2");

        let cfg = Config::from_getter(getter(m)).expect("should load");
        assert_eq!(
            cfg,
            Config {
                nats_url: "nats://nats.nats:4222".to_string(),
                lifecycle_stream: "STREAM_X".to_string(),
                lifecycle_subject: "x.lifecycle.>".to_string(),
                ticker_subject: "x.ticker.>".to_string(),
                gcs_bucket: "my-bucket".to_string(),
                gcs_prefix: "settled/x".to_string(),
                series_suffix: "5M".to_string(),
                redis_url: Some("redis://redis:6379".to_string()),
                database_url: Some("postgres://u:p@db/ssmd".to_string()),
                consumer_name: "snap-v2".to_string(),
            }
        );
    }

    #[test]
    fn empty_optional_falls_back_to_default_and_none() {
        let mut m = HashMap::new();
        m.insert("NATS_URL", "nats://localhost:4222");
        m.insert("GCS_BUCKET", "");
        m.insert("REDIS_URL", "");
        let cfg = Config::from_getter(getter(m)).expect("should load");
        assert_eq!(cfg.gcs_bucket, "ssmd-data");
        assert!(cfg.redis_url.is_none());
    }
}
