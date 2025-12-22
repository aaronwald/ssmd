use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::MetadataError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TransportType {
    Nats,
    Mqtt,
    Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StorageType {
    Local,
    S3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CacheType {
    Memory,
    Redis,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum KeyType {
    ApiKey,
    Transport,
    Storage,
    Tls,
    Webhook,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub timezone: Option<String>,
    pub day_start: Option<String>,
    pub day_end: Option<String>,
    pub auto_roll: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySpec {
    #[serde(rename = "type")]
    pub key_type: KeyType,
    pub description: Option<String>,
    pub required: Option<bool>,
    pub fields: Vec<String>,
    pub source: Option<String>,
    pub rotation_days: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    #[serde(rename = "type")]
    pub transport_type: TransportType,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(rename = "type")]
    pub storage_type: StorageType,
    pub path: Option<String>,
    pub bucket: Option<String>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(rename = "type")]
    pub cache_type: CacheType,
    pub max_size: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub feed: String,
    pub schema: String,
    pub schedule: Option<Schedule>,
    pub keys: Option<HashMap<String, KeySpec>>,
    pub transport: TransportConfig,
    pub storage: StorageConfig,
    pub cache: Option<CacheConfig>,
}

impl Environment {
    pub fn load(path: &Path) -> Result<Self, MetadataError> {
        let content = std::fs::read_to_string(path)?;
        let env: Environment = serde_yaml::from_str(&content)?;
        Ok(env)
    }

    /// Get the schema name (before the colon)
    pub fn get_schema_name(&self) -> &str {
        self.schema.split(':').next().unwrap_or(&self.schema)
    }

    /// Get the schema version (after the colon)
    pub fn get_schema_version(&self) -> &str {
        self.schema.split(':').nth(1).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_environment() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
name: kalshi-dev
feed: kalshi
schema: trade:v1
keys:
  kalshi:
    type: api_key
    fields:
      - api_key
      - api_secret
    source: env:KALSHI_API_KEY,KALSHI_API_SECRET
transport:
  type: memory
storage:
  type: local
  path: /var/lib/ssmd/data
"#
        )
        .unwrap();

        let env = Environment::load(file.path()).unwrap();
        assert_eq!(env.name, "kalshi-dev");
        assert_eq!(env.feed, "kalshi");
        assert_eq!(env.get_schema_name(), "trade");
        assert_eq!(env.get_schema_version(), "v1");
    }
}
