//! Kalshi configuration
//!
//! Loads Kalshi credentials from environment variables.

use std::env;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),

    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

/// Kalshi connector configuration
#[derive(Debug, Clone)]
pub struct KalshiConfig {
    pub api_key: String,
    pub private_key_pem: String,
    pub use_demo: bool,
}

impl KalshiConfig {
    /// Load configuration from environment variables
    ///
    /// Required:
    /// - `KALSHI_API_KEY`: API key from Kalshi
    /// - `KALSHI_PRIVATE_KEY`: PEM-encoded RSA private key (can include newlines)
    ///
    /// Optional:
    /// - `KALSHI_USE_DEMO`: Set to "true" to use demo API (default: false)
    pub fn from_env() -> Result<Self, ConfigError> {
        let api_key = env::var("KALSHI_API_KEY")
            .map_err(|_| ConfigError::MissingEnvVar("KALSHI_API_KEY".to_string()))?;

        let private_key_pem = env::var("KALSHI_PRIVATE_KEY")
            .map_err(|_| ConfigError::MissingEnvVar("KALSHI_PRIVATE_KEY".to_string()))?;

        let use_demo = env::var("KALSHI_USE_DEMO")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        Ok(Self {
            api_key,
            private_key_pem,
            use_demo,
        })
    }

    /// Create configuration with explicit values (for testing)
    pub fn new(api_key: String, private_key_pem: String, use_demo: bool) -> Self {
        Self {
            api_key,
            private_key_pem,
            use_demo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_new() {
        let config = KalshiConfig::new(
            "test-key".to_string(),
            "test-pem".to_string(),
            true,
        );

        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.private_key_pem, "test-pem");
        assert!(config.use_demo);
    }

    #[test]
    fn test_config_from_env_missing_api_key() {
        // Clear any existing env vars
        env::remove_var("KALSHI_API_KEY");
        env::remove_var("KALSHI_PRIVATE_KEY");

        let result = KalshiConfig::from_env();
        assert!(result.is_err());
        match result {
            Err(ConfigError::MissingEnvVar(var)) => {
                assert_eq!(var, "KALSHI_API_KEY");
            }
            _ => panic!("Expected MissingEnvVar error"),
        }
    }

    #[test]
    fn test_config_from_env_missing_private_key() {
        env::set_var("KALSHI_API_KEY", "test-key");
        env::remove_var("KALSHI_PRIVATE_KEY");

        let result = KalshiConfig::from_env();
        assert!(result.is_err());
        match result {
            Err(ConfigError::MissingEnvVar(var)) => {
                assert_eq!(var, "KALSHI_PRIVATE_KEY");
            }
            _ => panic!("Expected MissingEnvVar error"),
        }

        env::remove_var("KALSHI_API_KEY");
    }

    #[test]
    fn test_config_from_env_success() {
        env::set_var("KALSHI_API_KEY", "my-api-key");
        env::set_var("KALSHI_PRIVATE_KEY", "my-private-key");
        env::remove_var("KALSHI_USE_DEMO");

        let config = KalshiConfig::from_env().unwrap();
        assert_eq!(config.api_key, "my-api-key");
        assert_eq!(config.private_key_pem, "my-private-key");
        assert!(!config.use_demo);

        env::remove_var("KALSHI_API_KEY");
        env::remove_var("KALSHI_PRIVATE_KEY");
    }

    #[test]
    fn test_config_use_demo_parsing() {
        env::set_var("KALSHI_API_KEY", "key");
        env::set_var("KALSHI_PRIVATE_KEY", "pem");

        // Test "true"
        env::set_var("KALSHI_USE_DEMO", "true");
        assert!(KalshiConfig::from_env().unwrap().use_demo);

        // Test "TRUE"
        env::set_var("KALSHI_USE_DEMO", "TRUE");
        assert!(KalshiConfig::from_env().unwrap().use_demo);

        // Test "1"
        env::set_var("KALSHI_USE_DEMO", "1");
        assert!(KalshiConfig::from_env().unwrap().use_demo);

        // Test "false"
        env::set_var("KALSHI_USE_DEMO", "false");
        assert!(!KalshiConfig::from_env().unwrap().use_demo);

        // Test empty
        env::set_var("KALSHI_USE_DEMO", "");
        assert!(!KalshiConfig::from_env().unwrap().use_demo);

        env::remove_var("KALSHI_API_KEY");
        env::remove_var("KALSHI_PRIVATE_KEY");
        env::remove_var("KALSHI_USE_DEMO");
    }
}
