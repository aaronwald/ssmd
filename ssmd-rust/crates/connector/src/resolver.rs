use std::collections::HashMap;
use std::env;

use crate::error::ResolverError;
use crate::traits::KeyResolver;

/// Resolves keys from environment variables
pub struct EnvResolver;

impl EnvResolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnvResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyResolver for EnvResolver {
    /// Parses "env:VAR1,VAR2" and returns values from environment
    fn resolve(&self, source: &str) -> Result<HashMap<String, String>, ResolverError> {
        let prefix = "env:";
        if !source.starts_with(prefix) {
            return Err(ResolverError::UnsupportedSource(format!(
                "expected 'env:' prefix, got: {}",
                source
            )));
        }

        let vars_part = &source[prefix.len()..];
        if vars_part.is_empty() {
            return Err(ResolverError::UnsupportedSource(
                "empty env source".to_string(),
            ));
        }

        let mut result = HashMap::new();
        for var in vars_part.split(',') {
            let var = var.trim();
            if var.is_empty() {
                continue;
            }
            match env::var(var) {
                Ok(value) => {
                    result.insert(var.to_string(), value);
                }
                Err(_) => {
                    return Err(ResolverError::MissingKey(var.to_string()));
                }
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_env_vars() {
        env::set_var("TEST_KEY1", "value1");
        env::set_var("TEST_KEY2", "value2");

        let resolver = EnvResolver::new();
        let result = resolver.resolve("env:TEST_KEY1,TEST_KEY2").unwrap();

        assert_eq!(result.get("TEST_KEY1"), Some(&"value1".to_string()));
        assert_eq!(result.get("TEST_KEY2"), Some(&"value2".to_string()));

        env::remove_var("TEST_KEY1");
        env::remove_var("TEST_KEY2");
    }

    #[test]
    fn test_missing_var() {
        let resolver = EnvResolver::new();
        let result = resolver.resolve("env:NONEXISTENT_VAR_12345");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_source() {
        let resolver = EnvResolver::new();
        let result = resolver.resolve("vault:secret/path");
        assert!(result.is_err());
    }
}
