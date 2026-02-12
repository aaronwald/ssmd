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
}

/// Output format for archived data.
#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Jsonl,
    Parquet,
    Both,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Jsonl => write!(f, "jsonl"),
            OutputFormat::Parquet => write!(f, "parquet"),
            OutputFormat::Both => write!(f, "both"),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    pub path: PathBuf,
    /// Feed name for directory structure
    pub feed: String,
    /// Output format: "jsonl" (default), "parquet", or "both"
    #[serde(default)]
    pub format: OutputFormat,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RotationConfig {
    pub interval: String,
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, crate::ArchiverError> {
        let content = std::fs::read_to_string(path)?;
        serde_yaml::from_str(&content)
            .map_err(|e| crate::ArchiverError::Config(e.to_string()))
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
        let num: u64 = num_str.parse()
            .map_err(|_| crate::ArchiverError::Config(format!("Invalid interval: {}", s)))?;

        match unit {
            "s" => Ok(Duration::from_secs(num)),
            "m" => Ok(Duration::from_secs(num * 60)),
            "h" => Ok(Duration::from_secs(num * 60 * 60)),
            "d" => Ok(Duration::from_secs(num * 60 * 60 * 24)),
            _ => Err(crate::ArchiverError::Config(format!("Unknown unit: {}", unit))),
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
        // Default format when not specified
        assert_eq!(config.storage.format, OutputFormat::Jsonl);
    }

    #[test]
    fn test_load_config_parquet_format() {
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
  format: parquet

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.storage.format, OutputFormat::Parquet);
    }

    #[test]
    fn test_load_config_both_format() {
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
  format: both

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.storage.format, OutputFormat::Both);
    }

    #[test]
    fn test_output_format_display() {
        assert_eq!(OutputFormat::Jsonl.to_string(), "jsonl");
        assert_eq!(OutputFormat::Parquet.to_string(), "parquet");
        assert_eq!(OutputFormat::Both.to_string(), "both");
    }

    #[test]
    fn test_parse_interval() {
        let config = RotationConfig { interval: "15m".to_string() };
        assert_eq!(config.parse_interval().unwrap(), Duration::from_secs(15 * 60));

        let config = RotationConfig { interval: "1h".to_string() };
        assert_eq!(config.parse_interval().unwrap(), Duration::from_secs(60 * 60));

        let config = RotationConfig { interval: "1d".to_string() };
        assert_eq!(config.parse_interval().unwrap(), Duration::from_secs(24 * 60 * 60));
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
}
