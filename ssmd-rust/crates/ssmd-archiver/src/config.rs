use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub nats: NatsConfig,
    pub storage: StorageConfig,
    pub rotation: RotationConfig,
}

#[derive(Debug, Deserialize)]
pub struct NatsConfig {
    pub url: String,
    pub streams: Vec<StreamConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StreamConfig {
    /// Directory name for output (e.g., "politics")
    pub name: String,
    /// NATS JetStream stream name
    pub stream: String,
    /// Durable consumer name
    pub consumer: String,
    /// Subject filter pattern
    pub filter: String,
    /// Per-stream feed name (falls back to storage.feed if empty)
    #[serde(default)]
    pub feed: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    pub path: PathBuf,
    /// Global feed name (used as fallback when per-stream feed is not set)
    #[serde(default)]
    pub feed: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RotationConfig {
    pub interval: String,
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, crate::ArchiverError> {
        let content = std::fs::read_to_string(path)?;
        serde_yaml::from_str(&content).map_err(|e| crate::ArchiverError::Config(e.to_string()))
    }

    /// Fill empty per-stream feed names from the global storage.feed value.
    pub fn resolve_feeds(&mut self) {
        let global_feed = &self.storage.feed;
        for stream in &mut self.nats.streams {
            if stream.feed.is_empty() {
                stream.feed = global_feed.clone();
            }
        }
    }
}

impl RotationConfig {
    /// Parse interval string like "15m", "1h", "1d" to Duration
    pub fn parse_interval(&self) -> Result<Duration, crate::ArchiverError> {
        let s = self.interval.trim();
        if s.is_empty() {
            return Err(crate::ArchiverError::Config("Empty interval".to_string()));
        }

        let (num_str, unit) = s.split_at(s.len() - 1);
        let num: u64 = num_str
            .parse()
            .map_err(|_| crate::ArchiverError::Config(format!("Invalid interval: {}", s)))?;

        if num == 0 {
            return Err(crate::ArchiverError::Config(
                "Rotation interval must be greater than zero".to_string(),
            ));
        }

        match unit {
            "s" => Ok(Duration::from_secs(num)),
            "m" => Ok(Duration::from_secs(num * 60)),
            "h" => Ok(Duration::from_secs(num * 60 * 60)),
            "d" => Ok(Duration::from_secs(num * 60 * 60 * 24)),
            _ => Err(crate::ArchiverError::Config(format!(
                "Unknown unit: {}",
                unit
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config() {
        let yaml = r#"
nats:
  url: nats://localhost:4222
  streams:
    - name: main
      stream: MARKETDATA
      consumer: archiver-kalshi
      filter: "prod.kalshi.json.>"

storage:
  path: /data/ssmd
  feed: kalshi

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.nats.url, "nats://localhost:4222");
        assert_eq!(config.nats.streams.len(), 1);
        assert_eq!(config.nats.streams[0].stream, "MARKETDATA");
        assert_eq!(config.storage.feed, "kalshi");
        assert_eq!(config.rotation.interval, "15m");
    }

    #[test]
    fn test_parse_interval() {
        let config = RotationConfig {
            interval: "15m".to_string(),
        };
        assert_eq!(
            config.parse_interval().unwrap(),
            Duration::from_secs(15 * 60)
        );

        let config = RotationConfig {
            interval: "1h".to_string(),
        };
        assert_eq!(
            config.parse_interval().unwrap(),
            Duration::from_secs(60 * 60)
        );

        let config = RotationConfig {
            interval: "1d".to_string(),
        };
        assert_eq!(
            config.parse_interval().unwrap(),
            Duration::from_secs(24 * 60 * 60)
        );
    }

    #[test]
    fn test_parse_interval_rejects_zero() {
        let config = RotationConfig {
            interval: "0m".to_string(),
        };
        assert!(config.parse_interval().is_err());
    }

    #[test]
    fn test_load_multi_stream_config() {
        let yaml = r#"
nats:
  url: nats://localhost:4222
  streams:
    - name: politics
      stream: PROD_KALSHI_POLITICS
      consumer: politics-archiver
      filter: "prod.kalshi.politics.json.>"
    - name: economics
      stream: PROD_KALSHI_ECONOMICS
      consumer: economics-archiver
      filter: "prod.kalshi.economics.json.>"

storage:
  path: /data/ssmd
  feed: kalshi

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.nats.url, "nats://localhost:4222");
        assert_eq!(config.nats.streams.len(), 2);
        assert_eq!(config.nats.streams[0].name, "politics");
        assert_eq!(config.nats.streams[0].stream, "PROD_KALSHI_POLITICS");
        assert_eq!(config.nats.streams[1].name, "economics");
        assert_eq!(config.storage.feed, "kalshi");
    }

    #[test]
    fn test_config_ignores_unknown_fields() {
        // Existing configs may still have a "format" field — deserialization
        // should succeed (serde default is to ignore unknown fields with
        // deny_unknown_fields absent).
        let yaml = r#"
nats:
  url: nats://localhost:4222
  streams:
    - name: main
      stream: MARKETDATA
      consumer: archiver-kalshi
      filter: "prod.kalshi.json.>"

storage:
  path: /data/ssmd
  feed: kalshi
  format: jsonl

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.storage.feed, "kalshi");
    }

    #[test]
    fn test_load_per_stream_feed_config() {
        let yaml = r#"
nats:
  url: nats://localhost:4222
  streams:
    - name: crypto
      stream: PROD_KALSHI_CRYPTO
      consumer: archiver-kalshi
      filter: "prod.kalshi.json.>"
      feed: kalshi
    - name: futures
      stream: PROD_KRAKEN_FUTURES
      consumer: archiver-kraken
      filter: "prod.kraken-futures.json.>"
      feed: kraken-futures

storage:
  path: /data/ssmd

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.nats.streams.len(), 2);
        assert_eq!(config.nats.streams[0].feed, "kalshi");
        assert_eq!(config.nats.streams[1].feed, "kraken-futures");
        assert!(config.storage.feed.is_empty());
    }

    #[test]
    fn test_resolve_feeds_fallback() {
        let yaml = r#"
nats:
  url: nats://localhost:4222
  streams:
    - name: main
      stream: MARKETDATA
      consumer: archiver-kalshi
      filter: "prod.kalshi.json.>"

storage:
  path: /data/ssmd
  feed: kalshi

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let mut config = Config::load(file.path()).unwrap();
        assert!(config.nats.streams[0].feed.is_empty());
        config.resolve_feeds();
        assert_eq!(config.nats.streams[0].feed, "kalshi");
    }

    #[test]
    fn test_resolve_feeds_no_override() {
        let yaml = r#"
nats:
  url: nats://localhost:4222
  streams:
    - name: crypto
      stream: PROD_KALSHI_CRYPTO
      consumer: archiver-kalshi
      filter: "prod.kalshi.json.>"
      feed: kalshi
    - name: futures
      stream: PROD_KRAKEN_FUTURES
      consumer: archiver-kraken
      filter: "prod.kraken-futures.json.>"
      feed: kraken-futures

storage:
  path: /data/ssmd
  feed: fallback

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let mut config = Config::load(file.path()).unwrap();
        config.resolve_feeds();
        // Per-stream feeds should not be overridden by the global fallback
        assert_eq!(config.nats.streams[0].feed, "kalshi");
        assert_eq!(config.nats.streams[1].feed, "kraken-futures");
    }
}
