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
    /// NATS JetStream stream name (e.g., "PROD_KALSHI")
    pub stream: Option<String>,
    /// Subject prefix for NATS publishing (e.g., "prod.kalshi.main")
    /// If not set, defaults to "{env_name}.{feed_name}"
    pub subject_prefix: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecmasterConfig {
    pub url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    /// Only subscribe to markets closing within this many hours (for high-volume categories like Sports)
    #[serde(default)]
    pub close_within_hours: Option<u32>,
}

/// CDC configuration for dynamic market subscriptions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CdcConfig {
    /// Enable CDC dynamic subscriptions
    #[serde(default)]
    pub enabled: bool,
    /// NATS URL for CDC stream (defaults to transport.url if not set)
    #[serde(default)]
    pub nats_url: Option<String>,
    /// JetStream stream name (default: "SECMASTER_CDC")
    #[serde(default = "default_cdc_stream")]
    pub stream_name: String,
    /// Durable consumer name (should be unique per connector instance)
    /// If not set, defaults to "{connector_name}-cdc"
    #[serde(default)]
    pub consumer_name: Option<String>,
}

fn default_cdc_stream() -> String {
    "SECMASTER_CDC".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionConfig {
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_batch_delay_ms")]
    pub batch_delay_ms: u64,
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,
}

/// Default batch size for subscription requests
pub const DEFAULT_BATCH_SIZE: usize = 100;
/// Minimum allowed batch size
pub const MIN_BATCH_SIZE: usize = 1;
/// Maximum allowed batch size (Kalshi limit)
pub const MAX_BATCH_SIZE: usize = 500;
/// Default retry attempts for transient failures
pub const DEFAULT_RETRY_ATTEMPTS: u32 = 3;
/// Default retry delay in milliseconds
pub const DEFAULT_RETRY_DELAY_MS: u64 = 1000;
/// Default delay between subscription batches in milliseconds
pub const DEFAULT_BATCH_DELAY_MS: u64 = 1000;

fn default_batch_size() -> usize {
    DEFAULT_BATCH_SIZE
}

fn default_retry_attempts() -> u32 {
    DEFAULT_RETRY_ATTEMPTS
}

fn default_retry_delay_ms() -> u64 {
    DEFAULT_RETRY_DELAY_MS
}

fn default_batch_delay_ms() -> u64 {
    DEFAULT_BATCH_DELAY_MS
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        Self {
            batch_size: default_batch_size(),
            batch_delay_ms: default_batch_delay_ms(),
            retry_attempts: default_retry_attempts(),
            retry_delay_ms: default_retry_delay_ms(),
        }
    }
}

impl SubscriptionConfig {
    /// Validate the configuration, clamping batch_size to valid range.
    /// Returns a tuple of (validated_config, was_clamped).
    pub fn validated(mut self) -> (Self, bool) {
        let mut clamped = false;
        if self.batch_size < MIN_BATCH_SIZE {
            self.batch_size = MIN_BATCH_SIZE;
            clamped = true;
        } else if self.batch_size > MAX_BATCH_SIZE {
            self.batch_size = MAX_BATCH_SIZE;
            clamped = true;
        }
        (self, clamped)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub feed: String,
    pub schema: String,
    pub schedule: Option<Schedule>,
    pub keys: Option<HashMap<String, KeySpec>>,
    #[serde(default)]
    pub secmaster: Option<SecmasterConfig>,
    #[serde(default)]
    pub subscription: Option<SubscriptionConfig>,
    /// CDC configuration for dynamic market subscriptions
    #[serde(default)]
    pub cdc: Option<CdcConfig>,
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

    #[test]
    fn test_load_environment_with_secmaster() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
name: kalshi-politics
feed: kalshi
schema: trade:v1
secmaster:
  url: "http://ssmd-data-ts:3000"
  categories:
    - Politics
    - Economics
subscription:
  batch_size: 50
  retry_attempts: 5
transport:
  type: nats
  url: nats://localhost:4222
  stream: PROD_KALSHI_GOV
  subject_prefix: prod.kalshi.gov
storage:
  type: local
  path: /var/lib/ssmd/data
"#
        )
        .unwrap();

        let env = Environment::load(file.path()).unwrap();
        assert_eq!(env.name, "kalshi-politics");

        let secmaster = env.secmaster.unwrap();
        assert_eq!(secmaster.url, "http://ssmd-data-ts:3000");
        assert_eq!(secmaster.categories, vec!["Politics", "Economics"]);

        let subscription = env.subscription.unwrap();
        assert_eq!(subscription.batch_size, 50);
        assert_eq!(subscription.retry_attempts, 5);
    }

    #[test]
    fn test_subscription_config_validation_clamps_high() {
        let config = SubscriptionConfig {
            batch_size: 1000, // Above MAX_BATCH_SIZE
            batch_delay_ms: 1000,
            retry_attempts: 3,
            retry_delay_ms: 1000,
        };
        let (validated, clamped) = config.validated();
        assert_eq!(validated.batch_size, MAX_BATCH_SIZE);
        assert!(clamped);
    }

    #[test]
    fn test_subscription_config_validation_clamps_low() {
        let config = SubscriptionConfig {
            batch_size: 0, // Below MIN_BATCH_SIZE
            batch_delay_ms: 1000,
            retry_attempts: 3,
            retry_delay_ms: 1000,
        };
        let (validated, clamped) = config.validated();
        assert_eq!(validated.batch_size, MIN_BATCH_SIZE);
        assert!(clamped);
    }

    #[test]
    fn test_subscription_config_validation_keeps_valid() {
        let config = SubscriptionConfig {
            batch_size: 200,
            batch_delay_ms: 1000,
            retry_attempts: 3,
            retry_delay_ms: 1000,
        };
        let (validated, clamped) = config.validated();
        assert_eq!(validated.batch_size, 200);
        assert!(!clamped);
    }
}
