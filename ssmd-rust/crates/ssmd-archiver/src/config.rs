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
    pub stream: String,
    pub consumer: String,
    pub filter: String,
}

#[derive(Debug, Deserialize)]
pub struct StorageConfig {
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct RotationConfig {
    pub interval: String, // "15m", "1h", "1d"
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
  stream: MARKETDATA
  consumer: archiver-kalshi
  filter: "prod.kalshi.json.>"

storage:
  path: /data/ssmd

rotation:
  interval: 15m
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.nats.url, "nats://localhost:4222");
        assert_eq!(config.nats.stream, "MARKETDATA");
        assert_eq!(config.rotation.interval, "15m");
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
}
